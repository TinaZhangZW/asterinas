use core::{fmt::Display, sync::atomic::{AtomicBool, AtomicU32, Ordering}, time::Duration};

use aster_framebuffer::FRAMEBUFFER;
use aster_gpu::drm::{
    DrmError,
    driver::{DrmDriverFeatures, DumbCreateProvider},
    gem::{DrmGemBackend, DrmGemObject},
    ioctl::*,
    mode_config::{
        DrmModeModeInfo, DrmModeObject,
        property::{PropertyEnum, PropertyKind},
    },
};
use hashbrown::HashMap;
use ostd::mm::{VmIo, io_util::HasVmReaderWriter, HasPaddr, HasSize};
use ostd::sync::WaitQueue;
use ostd::Pod;
use crate::vm::vmo::{CommitFlags};
use aster_virtio::device::gpu::drm::gem::{
    VirtioGpuObjectParams, VirtioGpuSgEntry, VirtioGpuSgTable, virtio_gpu_blob_object_create, virtio_gpu_mode_dumb_create_with_sg,
    virtio_gpu_blob_mem_by_gem, virtio_gpu_blob_state_by_gem, virtio_gpu_create_hdr_by_gem,
    virtio_gpu_obj_resource_id, virtio_gpu_object_create, virtio_gpu_object_unref,
};

use crate::{
    current_userspace,
    device::drm::{
        ioctl_defs::*,
        memfd::{DrmMemfdFile, memfd_object_create},
        minor::DrmMinor,
    },
    events::IoEvents,
    fs::{
        file_handle::{FileLike, Mappable},
        file_table::FdFlags,
        inode_handle::FileIo,
        path::RESERVED_MOUNT_ID,
        pseudofs::AnonInodeFs,
        utils::{CreationFlags, Inode, InodeIo, StatusFlags},
    },
    prelude::*,
    process::{
        posix_thread::AsThreadLocal,
        signal::{PollHandle, Pollable},
    },
    time::{clocks::MonotonicClock, wait::WaitTimeout},
    util::ioctl::{RawIoctl, dispatch_ioctl},
};
use aster_virtio::device::gpu::{
    device::VirtioGpuDevice,
    drm as virtio_gpu_drm,
};

struct DrmSyncPoint {
    signaled: AtomicBool,
    wait_queue: WaitQueue,
    eventfd_listeners: Mutex<Vec<Arc<dyn FileLike>>>,
}

impl DrmSyncPoint {
    fn new(signaled: bool) -> Self {
        Self {
            signaled: AtomicBool::new(signaled),
            wait_queue: WaitQueue::new(),
            eventfd_listeners: Mutex::new(Vec::new()),
        }
    }

    fn is_signaled(&self) -> bool {
        self.signaled.load(Ordering::Acquire)
    }

    fn signal(&self) {
        self.signaled.store(true, Ordering::Release);
        self.wait_queue.wake_all();

        let listeners = core::mem::take(&mut *self.eventfd_listeners.lock());
        for eventfd in listeners {
            signal_eventfd(&eventfd);
        }
    }

    fn reset(&self) {
        self.signaled.store(false, Ordering::Release);
    }

    fn register_eventfd(&self, eventfd: Arc<dyn FileLike>) {
        if self.is_signaled() {
            signal_eventfd(&eventfd);
            return;
        }

        self.eventfd_listeners.lock().push(eventfd);
        if self.is_signaled() {
            let listeners = core::mem::take(&mut *self.eventfd_listeners.lock());
            for eventfd in listeners {
                signal_eventfd(&eventfd);
            }
        }
    }
}

struct DrmSyncObj {
    binary: Arc<DrmSyncPoint>,
    timeline: Mutex<HashMap<u64, Arc<DrmSyncPoint>>>,
    timeline_available_listeners: Mutex<HashMap<u64, Vec<Arc<dyn FileLike>>>>,
}

impl DrmSyncObj {
    fn new(initially_signaled: bool) -> Self {
        Self {
            binary: Arc::new(DrmSyncPoint::new(initially_signaled)),
            timeline: Mutex::new(HashMap::new()),
            timeline_available_listeners: Mutex::new(HashMap::new()),
        }
    }

    fn binary_point(&self) -> Arc<DrmSyncPoint> {
        self.binary.clone()
    }

    fn timeline_point(&self, point: u64, create: bool) -> Option<Arc<DrmSyncPoint>> {
        let mut timeline = self.timeline.lock();
        if let Some(existing) = timeline.get(&point) {
            return Some(existing.clone());
        }
        if !create {
            return None;
        }
        let entry = Arc::new(DrmSyncPoint::new(false));
        timeline.insert(point, entry.clone());
        let listeners = self.timeline_available_listeners.lock().remove(&point);
        drop(timeline);
        if let Some(listeners) = listeners {
            for eventfd in listeners {
                signal_eventfd(&eventfd);
            }
        }
        Some(entry)
    }

    fn highest_signaled_point(&self) -> u64 {
        let timeline = self.timeline.lock();
        timeline
            .iter()
            .filter_map(|(point, sync_point)| sync_point.is_signaled().then_some(*point))
            .max()
            .unwrap_or(0)
    }

    fn has_timeline_point(&self, point: u64) -> bool {
        self.timeline.lock().contains_key(&point)
    }

    fn register_timeline_available_eventfd(&self, point: u64, eventfd: Arc<dyn FileLike>) {
        if self.has_timeline_point(point) {
            signal_eventfd(&eventfd);
            return;
        }

        self.timeline_available_listeners
            .lock()
            .entry(point)
            .or_default()
            .push(eventfd);
    }
}

struct DrmSyncobjFdFile {
    syncobj: Arc<DrmSyncObj>,
}

const VIRTGPU_MAX_CAPSET_ID: u64 = 63;
const VIRTGPU_MAX_RINGS: u32 = 64;
const VIRTGPU_DEBUG_NAME_MAX_LEN: usize = 65;

#[derive(Debug)]
struct VirtioGpuContextState {
    ctx_id: u32,
    context_init: u32,
    context_created: bool,
    num_rings: u32,
    rings_initialized: bool,
    ring_idx_mask: u64,
    debug_name: [u8; VIRTGPU_DEBUG_NAME_MAX_LEN],
    debug_name_len: usize,
    explicit_debug_name: bool,
}

impl VirtioGpuContextState {
    const fn new() -> Self {
        Self {
            ctx_id: 0,
            context_init: 0,
            context_created: false,
            num_rings: 0,
            rings_initialized: false,
            ring_idx_mask: 0,
            debug_name: [0; VIRTGPU_DEBUG_NAME_MAX_LEN],
            debug_name_len: 0,
            explicit_debug_name: false,
        }
    }

    fn debug_name_bytes(&self) -> &[u8] {
        &self.debug_name[..self.debug_name_len]
    }
}

impl DrmSyncobjFdFile {
    fn new(syncobj: Arc<DrmSyncObj>) -> Self {
        Self { syncobj }
    }

    fn syncobj(&self) -> Arc<DrmSyncObj> {
        self.syncobj.clone()
    }
}

impl Pollable for DrmSyncobjFdFile {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        (IoEvents::IN | IoEvents::OUT) & mask
    }
}

impl FileLike for DrmSyncobjFdFile {
    fn inode(&self) -> &Arc<dyn Inode> {
        AnonInodeFs::shared_inode()
    }

    fn dump_proc_fdinfo(self: Arc<Self>, _fd_flags: FdFlags) -> Box<dyn Display> {
        struct FdInfo {
            ino: u64,
        }

        impl Display for FdInfo {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                writeln!(f, "pos:\t0")?;
                writeln!(f, "flags:\t02")?;
                writeln!(f, "mnt_id:\t{}", RESERVED_MOUNT_ID)?;
                writeln!(f, "ino:\t{}", self.ino)
            }
        }

        Box::new(FdInfo {
            ino: self.inode().ino(),
        })
    }
}

fn signal_eventfd(eventfd: &Arc<dyn FileLike>) {
    let one = 1u64.to_ne_bytes();
    let _ = eventfd.write_bytes(&one);
}

/// Represents an open DRM file descriptor exposed to userspace.
///
/// `DrmFile` is created on each successful `open()` of a DRM device node
/// (e.g. `/dev/dri/cardX`, `/dev/dri/renderDX`). It serves as the **per-open
/// execution context** for all userspace interactions with the DRM subsystem.
///
/// Responsibilities:
/// - Dispatching ioctl requests issued from userspace.
/// - Enforcing access restrictions and semantics defined by the associated
///   DRM minor (primary, render, control, etc.).
///
/// `DrmFile` does not own device-wide state. Instead, it holds a reference to
/// the `DrmMinor` through which it was opened, and all operations are ultimately
/// routed to the underlying `DrmDevice` shared by all minors of the same GPU.
///
/// Each `DrmFile` instance is independent and represents a single userspace
/// file descriptor, while the underlying DRM device and driver state are
/// shared across all open files.
pub(super) struct DrmFile {
    device: Arc<DrmMinor>,

    /// True when the client has asked us to expose stereo 3D mode flags.
    stereo_allowed: AtomicBool,
    /// True if client understands CRTC primary planes and cursor planes
    /// in the plane list. Automatically set when atomic is set.
    universal_planes: AtomicBool,
    /// True if client understands atomic properties.
    atomic: AtomicBool,
    /// True, if client can handle picture aspect ratios, and has requested
    /// to pass this information along with the mode.
    aspect_ratio_allowed: AtomicBool,
    /// True if client understands writeback connectors
    writeback_connectors: AtomicBool,
    /// This client is capable of handling the cursor plane with the
    /// restrictions imposed on it by the virtualized drivers.
    supports_virtualized_cursor_plane: AtomicBool,

    /// GEM objects are referenced by 32‑bit handles that
    /// are *per file descriptor*. Each open DRM file maintains its own
    /// namespace of GEM handles. This atomic counter is used to allocate
    /// unique handles for newly created GEM objects visible to userspace
    /// through this file.
    next_handle: AtomicU32,
    gem_table: Mutex<HashMap<u32, Arc<DrmGemObject>>>,
    gem_wait_table: Mutex<HashMap<u32, Arc<DrmSyncPoint>>>,
    gem_wait_queue: WaitQueue,

    next_syncobj_handle: AtomicU32,
    syncobj_table: Mutex<HashMap<u32, Arc<DrmSyncObj>>>,
    syncobj_wait_queue: WaitQueue,

    virtio_gpu_context: Mutex<VirtioGpuContextState>,
}

impl Pollable for DrmFile {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }
}

impl DrmFile {
    pub fn new(device: Arc<DrmMinor>) -> Self {
        Self {
            device,

            stereo_allowed: AtomicBool::new(false),
            universal_planes: AtomicBool::new(false),
            atomic: AtomicBool::new(false),
            aspect_ratio_allowed: AtomicBool::new(false),
            writeback_connectors: AtomicBool::new(false),
            supports_virtualized_cursor_plane: AtomicBool::new(false),

            next_handle: AtomicU32::new(1),
            gem_table: Mutex::new(HashMap::new()),
            gem_wait_table: Mutex::new(HashMap::new()),
            gem_wait_queue: WaitQueue::new(),

            next_syncobj_handle: AtomicU32::new(1),
            syncobj_table: Mutex::new(HashMap::new()),
            syncobj_wait_queue: WaitQueue::new(),

            virtio_gpu_context: Mutex::new(VirtioGpuContextState::new()),
        }
    }

    fn next_handle(&self) -> u32 {
        self.next_handle.fetch_add(1, Ordering::SeqCst)
    }

    fn insert_gem(&self, handle: u32, gem_object: Arc<DrmGemObject>) {
        self.gem_table.lock().insert(handle, gem_object);
        self.gem_wait_table
            .lock()
            .insert(handle, Arc::new(DrmSyncPoint::new(true)));
    }

    fn lookup_gem(&self, handle: &u32) -> Option<Arc<DrmGemObject>> {
        self.gem_table.lock().get(handle).cloned()
    }

    fn gem_map_offset(&self, handle: u32) -> Result<u64> {
        let gem_obj = self
            .lookup_gem(&handle)
            .ok_or_else(|| Error::new(Errno::ENOENT))?;

        // TODO: Track imported GEM objects and reject them here, matching
        // drm_gem_dumb_map_offset() semantics in Linux.
        Ok(self.device.create_offset(gem_obj))
    }

    fn lookup_gem_wait_point(&self, handle: &u32) -> Option<Arc<DrmSyncPoint>> {
        self.gem_wait_table.lock().get(handle).cloned()
    }

    fn reset_gem_wait_point(&self, handle: &u32) {
        if let Some(point) = self.lookup_gem_wait_point(handle) {
            point.reset();
        }
    }

    fn signal_gem_wait_point(&self, handle: &u32) {
        if let Some(point) = self.lookup_gem_wait_point(handle) {
            point.signal();
            self.gem_wait_queue.wake_all();
        }
    }

    fn next_syncobj_handle(&self) -> u32 {
        self.next_syncobj_handle.fetch_add(1, Ordering::SeqCst)
    }

    fn insert_syncobj(&self, handle: u32, syncobj: Arc<DrmSyncObj>) {
        self.syncobj_table.lock().insert(handle, syncobj);
    }

    fn lookup_syncobj(&self, handle: &u32) -> Option<Arc<DrmSyncObj>> {
        self.syncobj_table.lock().get(handle).cloned()
    }

    fn remove_syncobj(&self, handle: &u32) -> Option<Arc<DrmSyncObj>> {
        self.syncobj_table.lock().remove(handle)
    }

    fn read_user_array<T: Pod>(&self, ptr: u64, count: u32) -> Result<Vec<T>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        if ptr == 0 {
            return_errno!(Errno::EFAULT);
        }

        let mut values = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let offset = ptr as usize + i * core::mem::size_of::<T>();
            values.push(current_userspace!().read_val(offset)?);
        }
        Ok(values)
    }

    fn write_user_array<T: Pod>(&self, ptr: u64, values: &[T]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        if ptr == 0 {
            return_errno!(Errno::EFAULT);
        }

        for (i, value) in values.iter().enumerate() {
            let offset = ptr as usize + i * core::mem::size_of::<T>();
            current_userspace!().write_val(offset, value)?;
        }
        Ok(())
    }

    fn syncobj_deadline_to_duration(&self, timeout_nsec: u64) -> Option<Duration> {
        if timeout_nsec == u64::MAX {
            return None;
        }

        let now_ns = MonotonicClock::get().read_time().as_nanos() as u64;
        Some(Duration::from_nanos(timeout_nsec.saturating_sub(now_ns)))
    }

    fn gem_wait_common(&self, point: &Arc<DrmSyncPoint>, timeout: Option<Duration>) -> Result<bool> {
        if point.is_signaled() {
            return Ok(true);
        }

        let wait_res = self
            .gem_wait_queue
            .wait_until_or_timeout(|| point.is_signaled().then_some(()), timeout.as_ref());

        match wait_res {
            Ok(()) => Ok(true),
            Err(err) if err.error() == Errno::ETIME => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn syncobj_wait_common(
        &self,
        points: &[Arc<DrmSyncPoint>],
        wait_all: bool,
        timeout_nsec: u64,
    ) -> Result<Option<u32>> {
        let first_signaled = || -> Option<u32> {
            if wait_all {
                if points.iter().all(|point| point.is_signaled()) {
                    Some(0)
                } else {
                    None
                }
            } else {
                points.iter().position(|point| point.is_signaled()).map(|idx| idx as u32)
            }
        };

        if let Some(index) = first_signaled() {
            return Ok(Some(index));
        }

        let timeout = self.syncobj_deadline_to_duration(timeout_nsec);
        let wait_res = self
            .syncobj_wait_queue
            .wait_until_or_timeout(first_signaled, timeout.as_ref());

        match wait_res {
            Ok(index) => Ok(Some(index)),
            Err(err) if err.error() == Errno::ETIME => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn syncobj_timeline_point(
        &self,
        syncobj: &Arc<DrmSyncObj>,
        point: u64,
        wait_for_submit: bool,
    ) -> Result<Arc<DrmSyncPoint>> {
        match syncobj.timeline_point(point, wait_for_submit) {
            Some(sync_point) => Ok(sync_point),
            None => return_errno!(Errno::EINVAL),
        }
    }

    fn current_file_by_fd(&self, fd: i32) -> Result<Arc<dyn FileLike>> {
        if fd < 0 {
            return_errno!(Errno::EINVAL);
        }

        let current = ostd::task::Task::current().unwrap();
        let thread_local = current
            .as_thread_local()
            .ok_or_else(|| Error::new(Errno::ESRCH))?;

        let mut file_table = thread_local.borrow_file_table();
        let file = file_table.unwrap().read().get_file(fd)?.clone();
        Ok(file)
    }

    fn install_current_fd(&self, file: Arc<dyn FileLike>, cloexec: bool) -> Result<i32> {
        let current = ostd::task::Task::current().unwrap();
        let thread_local = current
            .as_thread_local()
            .ok_or_else(|| Error::new(Errno::ESRCH))?;

        let mut file_table = thread_local.borrow_file_table();
        let flags = if cloexec {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        Ok(file_table.unwrap().write().insert(file, flags))
    }

    fn virtio_gpu_param_value(&self, param: u64) -> Result<Option<u64>> {
        let gpu_device = self.device.gpu_device();
        let Ok(virtio_gpu) = Arc::downcast::<VirtioGpuDevice>(gpu_device) else {
            return Ok(None);
        };

        let value = match param {
            virtio_gpu_drm::VIRTGPU_PARAM_3D_FEATURES => u64::from(virtio_gpu.has_virgl_3d()),
            virtio_gpu_drm::VIRTGPU_PARAM_CAPSET_QUERY_FIX => {
                u64::from(virtio_gpu.num_capsets() > 0)
            }
            virtio_gpu_drm::VIRTGPU_PARAM_RESOURCE_BLOB => {
                u64::from(virtio_gpu.has_resource_blob())
            }
            virtio_gpu_drm::VIRTGPU_PARAM_HOST_VISIBLE => 0,
            virtio_gpu_drm::VIRTGPU_PARAM_CROSS_DEVICE => 0,
            virtio_gpu_drm::VIRTGPU_PARAM_CONTEXT_INIT => {
                u64::from(virtio_gpu.has_context_init() && virtio_gpu.has_virgl_3d())
            }
            virtio_gpu_drm::VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS => {
                self.virtio_gpu_supported_capset_mask(&virtio_gpu)
            }
            virtio_gpu_drm::VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME => {
                u64::from(virtio_gpu.has_context_init() && virtio_gpu.has_virgl_3d())
            }
            _ => {
                return_errno!(Errno::EINVAL);
            }
        };

        Ok(Some(value))
    }

    fn virtio_gpu_sg_from_backend(
        &self,
        backend: &Arc<dyn DrmGemBackend>,
    ) -> Result<VirtioGpuSgTable> {
        let drm_memfd = backend
            .downcast_ref::<DrmMemfdFile>()
            .ok_or_else(|| Error::new(Errno::EINVAL))?;
        let mapp = drm_memfd.mappable()?;
        let inode = match mapp {
            Mappable::Inode(inode) => inode,
            _ => return_errno!(Errno::EINVAL),
        };
        let vmo = inode
            .page_cache()
            .ok_or_else(|| Error::new(Errno::EINVAL))?;

        let byte_sz = vmo.size();
        let page_count = (byte_sz + ostd::mm::PAGE_SIZE - 1) / ostd::mm::PAGE_SIZE;

        // Gather physical pages and coalesce contiguous ranges for compact SG.
        let mut entries: Vec<VirtioGpuSgEntry> = Vec::new();
        for page_idx in 0..page_count {
            let frame = vmo.commit_on(page_idx, CommitFlags::empty())?;
            let p = frame.paddr();
            let s = frame.size();

            if let Some(last) = entries.last_mut() {
                let last_end = last.addr as usize + last.len as usize;
                if last_end == p {
                    last.len = last.len.saturating_add(s as u32);
                    continue;
                }
            }

            entries.push(VirtioGpuSgEntry {
                addr: p as u64,
                len: s as u32,
            });
        }

        Ok(VirtioGpuSgTable { entries })
    }

    fn virtio_gpu_device(&self) -> Option<Arc<VirtioGpuDevice>> {
        Arc::downcast::<VirtioGpuDevice>(self.device.gpu_device()).ok()
    }

    fn virtio_gpu_supported_capset_mask(&self, virtio_gpu: &VirtioGpuDevice) -> u64 {
        virtio_gpu.capset_infos().into_iter().fold(0u64, |supported, capset| {
            if capset.capset_id < u64::BITS {
                supported | (1u64 << capset.capset_id)
            } else {
                supported
            }
        })
    }

    fn read_user_cstring<const N: usize>(&self, ptr: u64) -> Result<([u8; N], usize)> {
        if ptr == 0 {
            return_errno!(Errno::EFAULT);
        }

        let mut out = [0u8; N];
        let mut len = 0usize;
        while len + 1 < N {
            let byte: u8 = current_userspace!().read_val(ptr as usize + len)?;
            if byte == 0 {
                return Ok((out, len));
            }
            out[len] = byte;
            len += 1;
        }

        let terminator: u8 = current_userspace!().read_val(ptr as usize + len)?;
        if terminator != 0 {
            return_errno!(Errno::EINVAL);
        }

        Ok((out, len))
    }

    fn ensure_virtio_gpu_context(&self, virtio_gpu: &Arc<VirtioGpuDevice>) -> Result<u32> {
        let mut state = self.virtio_gpu_context.lock();
        self.ensure_virtio_gpu_context_locked(virtio_gpu, &mut state)
    }

    fn ensure_virtio_gpu_context_locked(
        &self,
        virtio_gpu: &Arc<VirtioGpuDevice>,
        state: &mut VirtioGpuContextState,
    ) -> Result<u32> {
        if state.context_created {
            return Ok(state.ctx_id);
        }

        let ctx_id = if state.ctx_id != 0 {
            state.ctx_id
        } else {
            virtio_gpu.alloc_context_id()
        };

        virtio_gpu
            .context_create(ctx_id, state.context_init, state.debug_name_bytes())
            .map_err(|_| Error::new(Errno::EIO))?;

        state.ctx_id = ctx_id;
        state.context_created = true;
        Ok(ctx_id)
    }

    fn virtio_gpu_ring_idx(&self, requested: bool, ring_idx: u32) -> Result<Option<u8>> {
        if !requested {
            return Ok(None);
        }

        let state = self.virtio_gpu_context.lock();
        if !state.rings_initialized {
            return_errno!(Errno::EINVAL);
        }
        if ring_idx >= state.num_rings {
            return_errno!(Errno::EINVAL);
        }

        Ok(Some(u8::try_from(ring_idx).map_err(|_| Error::new(Errno::EINVAL))?))
    }

    fn virtio_gpu_capset_info(
        &self,
        capset_id: u32,
        capset_version: u32,
    ) -> Result<Option<aster_virtio::device::gpu::VirtioGpuRespCapsetInfo>> {
        let Some(virtio_gpu) = self.virtio_gpu_device() else {
            return Ok(None);
        };

        let capset = virtio_gpu.capset_infos().into_iter().find(|capset| {
            capset.capset_id == capset_id && capset_version <= capset.capset_max_version
        });

        Ok(capset)
    }

    fn remove_gem(&self, handle: &u32) -> Option<Arc<DrmGemObject>> {
        self.gem_wait_table.lock().remove(handle);
        self.gem_table.lock().remove(handle)
    }

    fn close_gem_handle(&self, handle: u32) -> Result<()> {
        let driver = self.device.driver();
        let driver_name = driver.name();

        let Some(gem_obj) = self.remove_gem(&handle) else {
            return_errno!(Errno::ENOENT);
        };

        // Keep virtio resource lifetime in sync with GEM handle lifetime.
        if driver_name == "virtio_gpu" {
            let _ = virtio_gpu_object_unref(&gem_obj);
        }

        let _ = gem_obj.release();
        self.device.remove_offset(&gem_obj);
        Ok(())
    }
}

impl InodeIo for DrmFile {
    fn read_at(
        &self,
        _offset: usize,
        _writer: &mut VmWriter,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        return_errno_with_message!(Errno::EINVAL, "drm: read not supported");
    }

    fn write_at(
        &self,
        _offset: usize,
        _reader: &mut VmReader,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        return_errno_with_message!(Errno::EINVAL, "drm: write not supported");
    }
}

impl FileIo for DrmFile {
    fn check_seekable(&self) -> Result<()> {
        Ok(())
    }

    fn is_offset_aware(&self) -> bool {
        true
    }

    fn mappable_with_offset(&self, offset: usize) -> Result<Mappable> {
        if let Some(gem_obj) = self.device.lookup_offset(&(offset as u64)) {
            if let Some(drm_memfd) = gem_obj.downcast_ref::<DrmMemfdFile>() {
                return drm_memfd.mappable();
            } else {
                // TODO: hardware memory mmap
            }
        }

        return_errno!(Errno::EINVAL);
    }

    fn ioctl(&self, raw_ioctl: RawIoctl) -> Result<i32> {
        // TODO: Call GpuDevice.handle_command() if it needs device specific ioctl handling.
        // TODO: drm_file permit flags check (master, root, render ...)
        dispatch_ioctl!(match raw_ioctl {
            cmd @ DrmIoctlVersion => {
                let mut user_data: DrmVersion = cmd.read()?;

                let driver = self.device.driver();

                let name = driver.name();
                let name_len = name.len() as u64;
                let desc = driver.desc();
                let desc_len = desc.len() as u64;
                let date = driver.date();
                let date_len = date.len() as u64;

                if user_data.is_first_call() {
                    user_data.name_len = name_len;
                    user_data.desc_len = desc_len;
                    user_data.date_len = date_len;

                    cmd.write(&user_data)?;
                } else {
                    // TODO: better write cstring method
                    // the name,desc,date now is u64, maybe should use cstring?
                    if user_data.name_len >= name_len {
                        current_userspace!()
                            .write_bytes(user_data.name as usize, name.as_bytes())?;
                    } else {
                        if user_data.name_len == 0 {
                            return Ok(0);
                        }
                        return_errno!(Errno::EINVAL);
                    }

                    if user_data.desc_len >= desc_len {
                        current_userspace!()
                            .write_bytes(user_data.desc as usize, desc.as_bytes())?;
                    } else {
                        if user_data.desc_len == 0 {
                            return Ok(0);
                        }
                        return_errno!(Errno::EINVAL);
                    }

                    if user_data.date_len >= date_len {
                        current_userspace!()
                            .write_bytes(user_data.date as usize, date.as_bytes())?;
                    } else {
                        if user_data.date_len == 0 {
                            return Ok(0);
                        }
                        return_errno!(Errno::EINVAL);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlGetCap => {
                let mut user_data: DrmGetCap = cmd.read()?;

                let cap = DrmCapabilities::try_from(user_data.capability)?;

                let value = match cap {
                    DrmCapabilities::TimestampMonotonic => 1,
                    DrmCapabilities::Prime => {
                        (DrmPrimeValue::IMPORT | DrmPrimeValue::EXPORT).bits()
                    }
                    DrmCapabilities::SyncObj => {
                        self.device.check_feature(DrmDriverFeatures::SYNCOBJ) as u64
                    }
                    DrmCapabilities::SyncObjTimeline => self
                        .device
                        .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                        as u64,
                    _ => {
                        if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }

                        let mode_config = &self.device.resources().lock();

                        match cap {
                            DrmCapabilities::DumbBuffer => {
                                self.device.driver().driver_ops().dumb_create.is_some() as u64
                            }
                            DrmCapabilities::VblankHighCrtc => 1,
                            DrmCapabilities::DumbPreferredDepth => {
                                mode_config.preferred_depth as u64
                            }
                            DrmCapabilities::DumbPreferShadow => mode_config.prefer_shadow as u64,
                            DrmCapabilities::AsyncPageFlip => mode_config.async_page_flip as u64,
                            DrmCapabilities::PageFlipTarget => {
                                // TODO: check if each crtc has func: page_flip_target
                                0
                            }
                            DrmCapabilities::CursorWidth => match mode_config.cursor_width {
                                0 => 64,
                                w => w as u64,
                            },
                            DrmCapabilities::CursorHeight => match mode_config.cursor_height {
                                0 => 64,
                                h => h as u64,
                            },
                            DrmCapabilities::Addfb2Modifiers => {
                                !mode_config.fb_modifiers_not_supported as u64
                            }
                            DrmCapabilities::CrtcInVblankEvent => 1,
                            DrmCapabilities::AtomicAsyncPageFlip => {
                                (self.device.check_feature(DrmDriverFeatures::ATOMIC)
                                    && mode_config.async_page_flip)
                                    as u64
                            }
                            _ => 0,
                        }
                    }
                };

                user_data.value = value;

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSetClientCap => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSetClientCap = cmd.read()?;

                match ClientCaps::try_from(user_data.capability)? {
                    ClientCaps::Stereo3D => match user_data.value {
                        0 | 1 => {
                            self.stereo_allowed
                                .store(user_data.value == 1, Ordering::Relaxed);
                        }
                        _ => return_errno!(Errno::EINVAL),
                    },
                    ClientCaps::UniversalPlane => {
                        match user_data.value {
                            0 | 1 => {
                                self.universal_planes
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::Atomic => {
                        if !self.device.check_feature(DrmDriverFeatures::ATOMIC) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }
                        // TODO: The modesetting DDX has a totally broken idea of atomic.
                        // if (current->comm[0] == 'X' && req->value == 1) {
                        // 	pr_info("broken atomic modeset userspace detected, disabling atomic\n");
                        //  return -EOPNOTSUPP;
                        // }

                        match user_data.value {
                            0 | 1 | 2 => {
                                let v = user_data.value;

                                self.atomic.store(v >= 1, Ordering::Relaxed);
                                self.universal_planes.store(v >= 1, Ordering::Relaxed);
                                self.aspect_ratio_allowed.store(v == 2, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        }
                    }
                    ClientCaps::AspectRatio => {
                        match user_data.value {
                            0 | 1 => {
                                self.aspect_ratio_allowed
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::WritebackConnectors => {
                        if !self.atomic.load(Ordering::Relaxed) {
                            return_errno!(Errno::EINVAL);
                        }

                        match user_data.value {
                            0 | 1 => {
                                self.writeback_connectors
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::CursorPlaneHostport => {
                        if !self.device.check_feature(DrmDriverFeatures::CURSOR_HOTSPOT) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }

                        if !self.atomic.load(Ordering::Relaxed) {
                            return_errno!(Errno::EINVAL);
                        }

                        match user_data.value {
                            0 | 1 => {
                                self.supports_virtualized_cursor_plane
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlGemClose => {
                let user_data: DrmGemClose = cmd.read()?;
                self.close_gem_handle(user_data.handle)?;
                Ok(0)
            }
            _cmd @ DrmIoctlSetMaster => {
                // TODO:
                Ok(0)
            }
            _cmd @ DrmIoctlDropMaster => {
                // TODO:
                Ok(0)
            }
            cmd @ DrmIoctlModeGetResources => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetResources = cmd.read()?;

                let res = self.device.resources().lock();

                let count_crtcs = res.count_crtcs();
                let count_encoders = res.count_encoders();
                let count_connectors = res.count_connectors();
                let count_fbs = res.count_framebuffers();

                if user_data.is_first_call() {
                    user_data.count_crtcs = count_crtcs;
                    user_data.count_encoders = count_encoders;
                    user_data.count_connectors = count_connectors;
                    user_data.count_fbs = count_fbs;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_connectors >= count_connectors {
                        for (i, id) in res.connectors_id().enumerate() {
                            let offset = user_data.connector_id_ptr as usize
                                + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_crtcs >= count_crtcs {
                        for (i, id) in res.crtcs_id().enumerate() {
                            let offset =
                                user_data.crtc_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_encoders >= count_encoders {
                        for (i, id) in res.encoders_id().enumerate() {
                            let offset =
                                user_data.encoder_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_fbs >= count_fbs {
                        for (i, id) in res.framebuffer_id().enumerate() {
                            let offset =
                                user_data.encoder_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetCrtc => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeCrtc = cmd.read()?;
                let crtc_id = user_data.crtc_id;
                let crtc = match self.device.resources().lock().get_crtc(&crtc_id) {
                    Some(c) => c,
                    None => {
                        return_errno!(Errno::ENOENT)
                    }
                };

                // TODO: Full mode validation and proper atomic handling:
                //
                // Current implementation only returns basic CRTC fields (gamma_size, fb_id, x/y).
                // It does not validate the mode, handle atomic commits, or propagate errors
                // for unsupported configurations. These behaviors are part of the standard
                // Linux DRM design and must be implemented for proper userspace interaction.
                user_data.gamma_size = crtc.gamma_size();
                user_data.fb_id = crtc.fb_id();
                (user_data.x, user_data.y) = crtc.xy();

                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeSetCrtc => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let crtc_req: DrmModeCrtc = cmd.read()?;
                let crtc_id = crtc_req.crtc_id;
                let fb_id = crtc_req.fb_id;

                let mode_config = self.device.resources().lock();
                let crtc = match mode_config.get_crtc(&crtc_id) {
                    Some(c) => c,
                    None => {
                        return_errno!(Errno::ENOENT)
                    }
                };
                let drm_framebuffer = match mode_config.lookup_framebuffer(&fb_id) {
                    Some(fb) => fb,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                crtc.funcs
                    .set_config(crtc.clone(), drm_framebuffer, &crtc_req)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeCursor => {
                let _user_data: DrmModeCursor = cmd.read()?;

                // TODO:
                // not support hardware cursor return ENXIO
                return_errno!(Errno::ENXIO);
            }
            cmd @ DrmIoctlModeCursor2 => {
                let _user_data: DrmModeCursor = cmd.read()?;

                // TODO:
                // not support hardware cursor return ENXIO
                return_errno!(Errno::ENXIO);
            }
            cmd @ DrmIoctlSetGamma => {
                let _user_data: DrmModeCrtcLut = cmd.read()?;

                // TODO:

                Ok(0)
            }
            cmd @ DrmIoctlModeGetEncoder => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetEncoder = cmd.read()?;
                let encoder_id = user_data.encoder_id;

                let encoder = match self.device.resources().lock().get_encoder(&encoder_id) {
                    Some(encoder) => encoder,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                // TODO: implement proper encoder state resolution including lease support.
                //
                // A lease allows a different DRM client (lessee) to take exclusive
                // control of certain objects. When querying the encoder’s current CRTC,
                // the core checks whether the file descriptor holds a lease on that CRTC.
                // If so, it returns the leased crtc_id;
                // otherwise it may return 0 (no binding).

                user_data.encoder_type = encoder.type_() as u32;
                user_data.encoder_id = encoder.id();
                user_data.possible_crtcs = encoder.possible_crtcs();
                user_data.possible_clones = encoder.possible_clones();

                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeGetConnector => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetConnector = cmd.read()?;
                let conn_id = user_data.connector_id;

                let conn = match self.device.resources().lock().get_connector(&conn_id) {
                    Some(conn) => conn,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_modes = conn.count_modes();
                let count_props = conn.count_props();
                let count_encoders = conn.count_encoders();

                if user_data.is_first_call() {
                    let mode_config = self.device.resources().lock();
                    let max_x = mode_config.max_width;
                    let max_y = mode_config.max_height;
                    drop(mode_config);
                    conn.funcs.fill_modes(max_x, max_y, conn.clone())?;

                    // update new infomation
                    let count_modes = conn.count_modes();
                    let count_encoders = conn.count_encoders();
                    user_data.count_modes = count_modes;
                    user_data.connection = conn.status() as u32;

                    user_data.count_props = count_props;
                    user_data.count_encoders = count_encoders;

                    user_data.connector_type = conn.type_() as u32;
                    user_data.connector_type_id = conn.type_id_();

                    user_data.mm_width = conn.mm_width();
                    user_data.mm_height = conn.mm_height();
                    user_data.subpixel = conn.subpixel_order();
                    user_data.pad = 0;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_modes >= count_modes {
                        for (i, mode) in conn.modes().iter().enumerate() {
                            let offset = user_data.modes_ptr as usize
                                + i * core::mem::size_of::<DrmModeModeInfo>();
                            current_userspace!().write_val(offset, mode)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_encoders >= count_encoders as u32 {
                        for (i, id) in conn.possible_encoders_id().enumerate() {
                            let offset =
                                user_data.encoders_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_props >= count_props {
                        for (i, (id, value)) in conn.properties().enumerate() {
                            let id_offset =
                                user_data.props_ptr as usize + i * core::mem::size_of::<u32>();
                            let value_offset = user_data.prop_values_ptr as usize
                                + i * core::mem::size_of::<u64>();
                            current_userspace!().write_val(id_offset, id)?;
                            current_userspace!().write_val(value_offset, value)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetProperty => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetProperty = cmd.read()?;
                let prop_id = user_data.prop_id;

                let property = match self.device.resources().lock().get_properties(&prop_id) {
                    Some(prop) => prop,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_values = property.count_values();
                let count_enum_blobs = property.count_enum_blobs();

                if user_data.is_first_call() {
                    user_data.name = property.name();
                    user_data.flags = property.flags();
                    user_data.count_values = count_values;
                    user_data.count_enum_blobs = count_enum_blobs;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_values < count_values
                        || user_data.count_enum_blobs < count_enum_blobs
                    {
                        return_errno!(Errno::EINVAL);
                    }

                    match property.kind() {
                        PropertyKind::Range { min, max } => {
                            let values = [*min, *max];
                            for (i, val) in values.iter().enumerate() {
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<u64>();
                                current_userspace!().write_val(offset, val)?;
                            }
                        }
                        PropertyKind::SignedRange { min, max } => {
                            let values = [*min, *max];
                            for (i, val) in values.iter().enumerate() {
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<i64>();
                                current_userspace!().write_val(offset, val)?;
                            }
                        }
                        PropertyKind::Enum(items) | PropertyKind::Bitmask(items) => {
                            for (i, (val, name)) in items.iter().enumerate() {
                                // set value
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<u64>();
                                current_userspace!().write_val(offset, val)?;

                                // set enum
                                let prop_enum = PropertyEnum::new(*val, name);
                                let enum_offset = user_data.enum_blob_ptr as usize
                                    + i * core::mem::size_of::<PropertyEnum>();
                                current_userspace!().write_val(enum_offset, &prop_enum)?;
                            }
                        }
                        PropertyKind::Blob(blob_id) => {
                            current_userspace!()
                                .write_val(user_data.values_ptr as usize, blob_id)?;
                        }
                        PropertyKind::Object(obj_type) => {
                            current_userspace!()
                                .write_val(user_data.values_ptr as usize, &(*obj_type as u32))?;
                        }
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeSetProperty => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let _user_data: DrmModeConnectorSetProperty = cmd.read()?;

                // TODO

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPropBlob => {
                // TODO: implement property blob lookup and data copy.
                //
                // In the Linux DRM implementation, MODE_GETPROPBLOB needs to:
                //   * lookup the blob object by id (drm_property_blob_lookup_blob())
                //   * copy the blob data to userspace if the provided buffer is large enough
                //   * update the returned length field to reflect actual blob size
                //
                // This is required to correctly support blob-type properties exposed to userspace (e.g., IN_FORMATS).
                // Currently this is a stub and does not perform any blob resolution or data transfer.
                let _user_data: DrmModeGetBlob = cmd.read()?;
                Ok(0)
            }
            cmd @ DrmIoctlModeAddFB => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeFBCmd = cmd.read()?;
                let handle = user_data.handle;

                if let Some(gem_obj) = self.lookup_gem(&handle) {
                    // TODO: format check && flag check

                    let mut mode_config = self.device.resources().lock();
                    // TODO: the create_framebuffer is provide from
                    // framebuffer.funcs.create()
                    let fb_id = mode_config.create_framebuffer(
                        user_data.width,
                        user_data.height,
                        user_data.pitch,
                        user_data.bpp,
                        gem_obj,
                    )?;

                    user_data.fb_id = fb_id;

                    cmd.write(&user_data)?;
                } else {
                    return_errno!(Errno::EINVAL)
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeRmFB => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmModeFBCmd = cmd.read()?;
                let fb_id = user_data.fb_id;

                let mut mode_config = self.device.resources().lock();
                let _ = mode_config.remove_framebuffer(&fb_id);

                Ok(0)
            }
            cmd @ DrmIoctlModeDirtyFb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmModeFbDirtyCmd = cmd.read()?;
                let fb_id = user_data.fb_id;

                // TODO: just legacy achievement
                if let Some(framebuffer) = FRAMEBUFFER.get() {
                    let iomem = framebuffer.io_mem();
                    let mut writer = iomem.writer().to_fallible();

                    let mode_config = self.device.resources().lock();
                    if let Some(drm_framebuffer) = mode_config.lookup_framebuffer(&fb_id) {
                        // TODO: handle the error
                        let _ = drm_framebuffer.read(0, &mut writer);
                    } else {
                        return_errno!(Errno::ENOENT);
                    }
                } else {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeCreateDumb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeCreateDumb = cmd.read()?;
                let driver = self.device.driver();
                let driver_name = driver.name();

                if let Some(dumb_create) = driver.driver_ops().dumb_create {
                    // TODO: handle the error
                    let gem = if driver_name == "virtio_gpu" {
                        // For virtio-gpu: pre-allocate memfd and build SG before create.
                        // This keeps filesystem knowledge in file.rs.
                        if user_data.bpp != 32 {
                            return_errno!(Errno::EINVAL);
                        }

                        let pitch = user_data.width.saturating_mul(4);
                        let size_u64 = (pitch as u64).saturating_mul(user_data.height as u64);
                        let backend = memfd_object_create("virtio-gpu-dumb", size_u64)?;

                        let sg = self.virtio_gpu_sg_from_backend(&backend)?;

                        virtio_gpu_mode_dumb_create_with_sg(&mut user_data, backend, Some(&sg))?
                    } else {
                        match dumb_create {
                            DumbCreateProvider::MemfdBackend(dumb_create_impl) => {
                                dumb_create_impl(&mut user_data, memfd_object_create)?
                            }
                            DumbCreateProvider::Custom(dumb_create_impl) => {
                                dumb_create_impl(&mut user_data)?
                            }
                        }
                    };
                    let handle = self.next_handle();
                    user_data.handle = handle;

                    self.insert_gem(handle, gem.clone());

                    cmd.write(&user_data)?;
                } else {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeMapDumb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeMapDumb = cmd.read()?;
                let handle = user_data.handle;

                if self.device.driver().driver_ops().dumb_create.is_none() {
                    return_errno!(Errno::ENOSYS);
                }

                user_data.offset = self.gem_map_offset(handle)?;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeDestroyDumb => {
                if self.device.driver().driver_ops().dumb_create.is_none() {
                    return_errno!(Errno::ENOSYS);
                }

                let user_data: DrmModeDestroyDumb = cmd.read()?;
                self.close_gem_handle(user_data.handle)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPlaneResources => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetPlaneRes = cmd.read()?;
                let count_planes = self.device.resources().lock().count_planes();

                if user_data.is_first_call() {
                    user_data.count_planes = count_planes;
                    cmd.write(&user_data)?;
                } else {
                    // TODO: apply legacy plane filtering per client capabilities.
                    //
                    // Linux DRM only advertises overlay planes by default for legacy userspace.
                    // If the client has enabled the `DRM_CLIENT_CAP_UNIVERSAL_PLANES` cap (or
                    // supports atomic), primary and cursor planes should also be exposed.
                    // See drm_for_each_plane() and the handling of `file_priv->universal_planes`
                    // in the C implementation.

                    if user_data.count_planes >= count_planes {
                        for (i, id) in self.device.resources().lock().planes_id().enumerate() {
                            let offset =
                                user_data.plane_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPlane => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetPlane = cmd.read()?;
                let plane_id = user_data.plane_id;

                let _plane = match self.device.resources().lock().get_plane(&plane_id) {
                    Some(plane) => plane,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                // TODO: support state and format querying per Linux DRM semantics.
                //
                // The Linux DRM GETPLANE ioctl returns a plane’s current state in addition
                // to basic identifiers. In a full implementation, userspace expects:
                //
                //   * CRTC/fb binding from the current atomic or legacy plane state.
                //   * Plane formats and format count via `count_format_types`/`format_type_ptr`.
                //   * Checks for atomic capability and client caps (e.g., DRM_CLIENT_CAP_ATOMIC).
                //
                // At minimum, atomic state lookup must be done to fill `crtc_id`, `fb_id`,
                // and format lists per current plane state. This stub only zeroes gamma_size.

                user_data.gamma_size = 0;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeObjectGetProps => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeObjectGetProps = cmd.read()?;
                let obj_id = user_data.obj_id;

                let obj = match self.device.resources().lock().get_object(&obj_id) {
                    Some(o) => o,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_props = obj.count_props();

                if user_data.is_first_call() {
                    user_data.count_props = count_props;
                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_props >= count_props {
                        for (i, (id, value)) in obj.get_properties().enumerate() {
                            let id_offset =
                                user_data.props_ptr as usize + i * core::mem::size_of::<u32>();
                            let value_offset = user_data.prop_values_ptr as usize
                                + i * core::mem::size_of::<u64>();

                            current_userspace!().write_val(id_offset, &id)?;
                            current_userspace!().write_val(value_offset, &value)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuExecbuffer => {
                let mut user_data: aster_virtio::device::gpu::drm::VirtioGpuExecbuffer = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };
                if !virtio_gpu.has_virgl_3d() {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                if (user_data.flags & !virtio_gpu_drm::VIRTGPU_EXECBUF_FLAGS) != 0 {
                    return_errno!(Errno::EINVAL);
                }
                if (user_data.flags
                    & (virtio_gpu_drm::VIRTGPU_EXECBUF_FENCE_FD_IN
                        | virtio_gpu_drm::VIRTGPU_EXECBUF_FENCE_FD_OUT))
                    != 0
                {
                    return_errno!(Errno::EOPNOTSUPP);
                }
                if user_data.size == 0 || user_data.command == 0 {
                    return_errno!(Errno::EINVAL);
                }

                let mut command = vec![0u8; user_data.size as usize];
                current_userspace!().read_bytes(user_data.command as usize, &mut command)?;

                let bo_handles: Vec<u32> =
                    self.read_user_array(user_data.bo_handles, user_data.num_bo_handles)?;
                for bo_handle in &bo_handles {
                    if self.lookup_gem(&bo_handle).is_none() {
                        return_errno!(Errno::ENOENT);
                    }
                }

                let syncobj_stride = core::mem::size_of::<
                    aster_virtio::device::gpu::drm::VirtioGpuExecbufferSyncobj,
                >() as u32;
                if (user_data.num_in_syncobjs > 0 || user_data.num_out_syncobjs > 0)
                    && user_data.syncobj_stride != syncobj_stride
                {
                    return_errno!(Errno::EINVAL);
                }

                let in_syncobjs: Vec<aster_virtio::device::gpu::drm::VirtioGpuExecbufferSyncobj> =
                    self.read_user_array(user_data.in_syncobjs, user_data.num_in_syncobjs)?;
                let out_syncobjs: Vec<aster_virtio::device::gpu::drm::VirtioGpuExecbufferSyncobj> =
                    self.read_user_array(user_data.out_syncobjs, user_data.num_out_syncobjs)?;

                for sync in in_syncobjs.iter().chain(out_syncobjs.iter()) {
                    if (sync.flags & !virtio_gpu_drm::VIRTGPU_EXECBUF_SYNCOBJ_FLAGS) != 0 {
                        return_errno!(Errno::EINVAL);
                    }
                }

                for sync in &in_syncobjs {
                    let syncobj = self
                        .lookup_syncobj(&sync.handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;
                    if sync.point == 0 {
                        let point = syncobj.binary_point();
                        let _ = self.syncobj_wait_common(&[point.clone()], true, u64::MAX)?;
                        if (sync.flags & virtio_gpu_drm::VIRTGPU_EXECBUF_SYNCOBJ_RESET) != 0 {
                            point.reset();
                        }
                    } else {
                        let point = self.syncobj_timeline_point(&syncobj, sync.point, true)?;
                        let _ = self.syncobj_wait_common(&[point.clone()], true, u64::MAX)?;
                        if (sync.flags & virtio_gpu_drm::VIRTGPU_EXECBUF_SYNCOBJ_RESET) != 0 {
                            point.reset();
                        }
                    }
                }

                let ctx_id = self.ensure_virtio_gpu_context(&virtio_gpu)?;
                let ring_idx = self.virtio_gpu_ring_idx(
                    (user_data.flags & virtio_gpu_drm::VIRTGPU_EXECBUF_RING_IDX) != 0,
                    user_data.ring_idx,
                )?;

                for bo_handle in &bo_handles {
                    self.reset_gem_wait_point(bo_handle);
                }

                let submit_result = virtio_gpu
                    .submit_3d(&command, ctx_id, ring_idx)
                    .map_err(|_| Error::new(Errno::EIO));

                for bo_handle in &bo_handles {
                    self.signal_gem_wait_point(bo_handle);
                }

                let fence_id = submit_result?;

                for sync in &out_syncobjs {
                    let syncobj = self
                        .lookup_syncobj(&sync.handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;

                    let effective_point = if sync.point == 0 { fence_id } else { sync.point };
                    let point = syncobj
                        .timeline_point(effective_point, true)
                        .ok_or_else(|| Error::new(Errno::EINVAL))?;
                    if (sync.flags & virtio_gpu_drm::VIRTGPU_EXECBUF_SYNCOBJ_RESET) != 0 {
                        point.reset();
                    }
                    point.signal();

                    if effective_point != fence_id {
                        syncobj.timeline.lock().insert(fence_id, point.clone());
                    }
                    if sync.point == 0 {
                        syncobj.binary.signal();
                    }
                }
                self.syncobj_wait_queue.wake_all();

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSyncobjCreate => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmSyncobjCreate = cmd.read()?;
                if user_data.flags & !DRM_SYNCOBJ_CREATE_SIGNALED != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let handle = self.next_syncobj_handle();
                let initially_signaled =
                    (user_data.flags & DRM_SYNCOBJ_CREATE_SIGNALED) != 0;
                self.insert_syncobj(handle, Arc::new(DrmSyncObj::new(initially_signaled)));

                user_data.handle = handle;
                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSyncobjDestroy => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjDestroy = cmd.read()?;
                if self.remove_syncobj(&user_data.handle).is_none() {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjHandleToFd => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmSyncobjHandle = cmd.read()?;
                let known_flags = DRM_SYNCOBJ_HANDLE_TO_FD_FLAGS_EXPORT_SYNC_FILE
                    | CreationFlags::O_CLOEXEC.bits();
                if user_data.flags & !known_flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let syncobj = self
                    .lookup_syncobj(&user_data.handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;
                let fd_file: Arc<dyn FileLike> = Arc::new(DrmSyncobjFdFile::new(syncobj));

                let cloexec = (user_data.flags & CreationFlags::O_CLOEXEC.bits()) != 0;
                let fd = self.install_current_fd(fd_file, cloexec)?;
                user_data.fd = fd;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjFdToHandle => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmSyncobjHandle = cmd.read()?;
                let known_flags = DRM_SYNCOBJ_FD_TO_HANDLE_FLAGS_IMPORT_SYNC_FILE;
                if user_data.flags & !known_flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let file = self.current_file_by_fd(user_data.fd)?;
                let syncobj_file = file
                    .downcast_ref::<DrmSyncobjFdFile>()
                    .ok_or_else(|| Error::new(Errno::EINVAL))?;

                let handle = self.next_syncobj_handle();
                self.insert_syncobj(handle, syncobj_file.syncobj());
                user_data.handle = handle;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjWait => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmSyncobjWait = cmd.read()?;
                let known_flags =
                    DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL | DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT;
                if user_data.flags & !known_flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                if handles.is_empty() {
                    return_errno!(Errno::EINVAL);
                }

                let points = handles
                    .iter()
                    .map(|handle| {
                        let syncobj = self
                            .lookup_syncobj(handle)
                            .ok_or_else(|| Error::new(Errno::ENOENT))?;
                        Ok(syncobj.binary_point())
                    })
                    .collect::<Result<Vec<_>>>()?;

                let wait_all = (user_data.flags & DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL) != 0;
                let first_signaled = self.syncobj_wait_common(&points, wait_all, user_data.timeout_nsec)?;
                let Some(index) = first_signaled else {
                    return_errno!(Errno::ETIME);
                };

                user_data.first_signaled = index;
                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSyncobjReset => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjArray = cmd.read()?;
                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                for handle in handles {
                    let syncobj = self
                        .lookup_syncobj(&handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;
                    syncobj.binary.reset();
                }

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjSignal => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjArray = cmd.read()?;
                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                for handle in handles {
                    let syncobj = self
                        .lookup_syncobj(&handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;
                    syncobj.binary.signal();
                }
                self.syncobj_wait_queue.wake_all();

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjTimelineWait => {
                if !self
                    .device
                    .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmSyncobjTimelineWait = cmd.read()?;
                let known_flags = DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL
                    | DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT
                    | DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE;
                if user_data.flags & !known_flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                let points_in: Vec<u64> = self.read_user_array(user_data.points, user_data.count_handles)?;
                if handles.is_empty() {
                    return_errno!(Errno::EINVAL);
                }

                let wait_for_submit =
                    (user_data.flags & DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT) != 0;
                let wait_all = (user_data.flags & DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL) != 0;
                let wait_available =
                    (user_data.flags & DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE) != 0;

                let first_signaled = if wait_available {
                    let entries = handles
                        .iter()
                        .zip(points_in.iter())
                        .map(|(handle, point)| {
                            let syncobj = self
                                .lookup_syncobj(handle)
                                .ok_or_else(|| Error::new(Errno::ENOENT))?;
                            Ok((syncobj, *point))
                        })
                        .collect::<Result<Vec<_>>>()?;

                    let first_available = || -> Option<u32> {
                        if wait_all {
                            if entries
                                .iter()
                                .all(|(syncobj, point)| syncobj.has_timeline_point(*point))
                            {
                                Some(0)
                            } else {
                                None
                            }
                        } else {
                            entries
                                .iter()
                                .position(|(syncobj, point)| syncobj.has_timeline_point(*point))
                                .map(|idx| idx as u32)
                        }
                    };

                    if let Some(index) = first_available() {
                        Some(index)
                    } else {
                        let timeout = self.syncobj_deadline_to_duration(user_data.timeout_nsec);
                        match self
                            .syncobj_wait_queue
                            .wait_until_or_timeout(first_available, timeout.as_ref())
                        {
                            Ok(index) => Some(index),
                            Err(err) if err.error() == Errno::ETIME => None,
                            Err(err) => return Err(err),
                        }
                    }
                } else {
                    let points = handles
                        .iter()
                        .zip(points_in.iter())
                        .map(|(handle, point)| {
                            let syncobj = self
                                .lookup_syncobj(handle)
                                .ok_or_else(|| Error::new(Errno::ENOENT))?;
                            self.syncobj_timeline_point(&syncobj, *point, wait_for_submit)
                        })
                        .collect::<Result<Vec<_>>>()?;
                    self.syncobj_wait_common(&points, wait_all, user_data.timeout_nsec)?
                };

                let Some(index) = first_signaled else { return_errno!(Errno::ETIME); };

                user_data.first_signaled = index;
                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSyncobjQuery => {
                if !self
                    .device
                    .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjTimelineArray = cmd.read()?;
                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                let mut out_points: Vec<u64> = Vec::with_capacity(handles.len());

                for handle in handles {
                    let syncobj = self
                        .lookup_syncobj(&handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;
                    out_points.push(syncobj.highest_signaled_point());
                }

                self.write_user_array::<u64>(user_data.points, out_points.as_slice())?;
                Ok(0)
            }
            cmd @ DrmIoctlSyncobjTransfer => {
                if !self
                    .device
                    .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjTransfer = cmd.read()?;
                if user_data.flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let src_syncobj = self
                    .lookup_syncobj(&user_data.src_handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;
                let dst_syncobj = self
                    .lookup_syncobj(&user_data.dst_handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;

                let src_point = src_syncobj
                    .timeline_point(user_data.src_point, false)
                    .ok_or_else(|| Error::new(Errno::EINVAL))?;
                dst_syncobj
                    .timeline
                    .lock()
                    .insert(user_data.dst_point, src_point);

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjTimelineSignal => {
                if !self
                    .device
                    .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjTimelineArray = cmd.read()?;
                let handles: Vec<u32> = self.read_user_array(user_data.handles, user_data.count_handles)?;
                let points_in: Vec<u64> = self.read_user_array(user_data.points, user_data.count_handles)?;

                for (handle, point) in handles.iter().zip(points_in.iter()) {
                    let syncobj = self
                        .lookup_syncobj(handle)
                        .ok_or_else(|| Error::new(Errno::ENOENT))?;
                    let sync_point = syncobj
                        .timeline_point(*point, true)
                        .ok_or_else(|| Error::new(Errno::EINVAL))?;
                    sync_point.signal();
                }
                self.syncobj_wait_queue.wake_all();

                Ok(0)
            }
            cmd @ DrmIoctlSyncobjEventfd => {
                if !self.device.check_feature(DrmDriverFeatures::SYNCOBJ) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSyncobjEventfd = cmd.read()?;
                let known_flags = DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE;
                if user_data.flags & !known_flags != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let syncobj = self
                    .lookup_syncobj(&user_data.handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;
                let eventfd = self.current_file_by_fd(user_data.fd)?;

                let wait_available =
                    (user_data.flags & DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE) != 0;
                if wait_available {
                    if user_data.point == 0 {
                        signal_eventfd(&eventfd);
                    } else {
                        syncobj.register_timeline_available_eventfd(user_data.point, eventfd);
                    }
                    return Ok(0);
                }

                if user_data.point == 0 {
                    syncobj.binary_point().register_eventfd(eventfd);
                } else {
                    let point = syncobj
                        .timeline_point(user_data.point, true)
                        .ok_or_else(|| Error::new(Errno::EINVAL))?;
                    point.register_eventfd(eventfd);
                }

                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuGetParam => {
                let mut user_data = cmd.read()?;

                let value = match self.virtio_gpu_param_value(user_data.param)? {
                    Some(value) => value,
                    None => {
                        return_errno!(Errno::ENOTTY);
                    }
                };

                user_data.value = value;
                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuResourceCreate => {
                let mut user_data: aster_virtio::device::gpu::drm::VirtioGpuResourceCreate =
                    cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                if user_data.width == 0 || user_data.height == 0 {
                    return_errno!(Errno::EINVAL);
                }

                if !virtio_gpu.has_virgl_3d() {
                    // Match Linux's non-virgl restrictions for legacy 2D resource_create.
                    if user_data.depth > 1
                        || user_data.nr_samples > 1
                        || user_data.last_level > 1
                        || user_data.target != 2
                        || user_data.array_size > 1
                    {
                        return_errno!(Errno::EINVAL);
                    }
                } else {
                    let _ = self.ensure_virtio_gpu_context(&virtio_gpu)?;
                }

                let size = if user_data.size == 0 {
                    ostd::mm::PAGE_SIZE as u64
                } else {
                    user_data.size as u64
                };

                let params = VirtioGpuObjectParams {
                    virgl: virtio_gpu.has_virgl_3d(),
                    target: user_data.target,
                    format: user_data.format,
                    bind: user_data.bind,
                    width: user_data.width,
                    height: user_data.height,
                    depth: user_data.depth,
                    array_size: user_data.array_size,
                    last_level: user_data.last_level,
                    nr_samples: user_data.nr_samples,
                    flags: user_data.flags,
                    ..Default::default()
                };

                let backend = memfd_object_create("virtio-gpu-resource", size)?;
                let sg = self.virtio_gpu_sg_from_backend(&backend)?;

                // `virtio_gpu_object_create` submits a fenced control command and waits
                // for completion, so creation is ordered similarly to Linux's fenced path.
                let gem_obj = virtio_gpu_object_create(
                    size,
                    user_data.stride,
                    user_data.width,
                    user_data.height,
                    Some(&sg),
                    backend,
                    &params,
                )?;

                // Linux allocates and attaches a fence to resource creation.
                // Our create path is synchronous, but we still capture and consume
                // the virtio fence metadata for parity with that model.
                let create_hdr =
                    virtio_gpu_create_hdr_by_gem(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;
                if create_hdr.fence_id == 0 {
                    return_errno!(Errno::EIO);
                }

                let handle = self.next_handle();
                self.insert_gem(handle, gem_obj.clone());

                // Linux returns qobj->hw_res_handle as res_handle.
                let hw_res_handle =
                    virtio_gpu_obj_resource_id(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;

                user_data.bo_handle = handle;
                user_data.res_handle = hw_res_handle;
                user_data.size = u32::try_from(size).map_err(|_| Error::new(Errno::EOVERFLOW))?;

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuGetCaps => {
                let mut user_data = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                // If the device reports no capsets, behave like Linux and return ENOSYS.
                if virtio_gpu.num_capsets() == 0 {
                    println!("virtio-gpu: device has no capsets, rejecting GET_CAPS");
                    return_errno!(Errno::ENOSYS);
                }

                // Userspace must not pass size == 0.
                if user_data.size == 0 {
                    return_errno!(Errno::EINVAL);
                }

                let Some(capset_info) = self
                    .virtio_gpu_capset_info(user_data.cap_set_id, user_data.cap_set_ver)?
                else {
                    return_errno!(Errno::EINVAL);
                };

                let host_caps_size = capset_info.capset_max_size;
                let size_to_copy = core::cmp::min(user_data.size, host_caps_size) as usize;

                let caps = match virtio_gpu.get_capset(
                    capset_info.capset_id,
                    user_data.cap_set_ver,
                    capset_info.capset_max_size,
                ) {
                    Ok(v) => v,
                    Err(_) => return_errno!(Errno::EIO),
                };

                let copy_len = core::cmp::min(size_to_copy, caps.len());
                if copy_len > 0 {
                    current_userspace!().write_bytes(user_data.addr as usize, &caps[..copy_len])?;
                }

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuResourceCreateBlob => {
                let mut user_data: aster_virtio::device::gpu::drm::VirtioGpuResourceCreateBlob =
                    cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                if !virtio_gpu.has_resource_blob() {
                    return_errno!(Errno::EINVAL);
                }
                if (user_data.blob_flags & !virtio_gpu_drm::VIRTGPU_BLOB_FLAG_USE_MASK) != 0 {
                    return_errno!(Errno::EINVAL);
                }
                if (user_data.blob_flags & virtio_gpu_drm::VIRTGPU_BLOB_FLAG_USE_CROSS_DEVICE) != 0 {
                    return_errno!(Errno::EINVAL);
                }

                let (guest_blob, host3d_blob) = match user_data.blob_mem {
                    virtio_gpu_drm::VIRTGPU_BLOB_MEM_GUEST => (true, false),
                    virtio_gpu_drm::VIRTGPU_BLOB_MEM_HOST3D => (false, true),
                    virtio_gpu_drm::VIRTGPU_BLOB_MEM_HOST3D_GUEST => (true, true),
                    _ => return_errno!(Errno::EINVAL),
                };

                if host3d_blob {
                    if !virtio_gpu.has_virgl_3d() {
                        return_errno!(Errno::EINVAL);
                    }
                    if user_data.cmd_size % 4 != 0 {
                        return_errno!(Errno::EINVAL);
                    }
                } else if user_data.blob_id != 0 || user_data.cmd_size != 0 {
                    return_errno!(Errno::EINVAL);
                }

                if !guest_blob {
                    return_errno!(Errno::EINVAL);
                }

                if user_data.cmd_size != 0 {
                    let ctx_id = self.ensure_virtio_gpu_context(&virtio_gpu)?;
                    let mut command = vec![0u8; user_data.cmd_size as usize];
                    current_userspace!().read_bytes(user_data.cmd as usize, &mut command)?;
                    virtio_gpu
                        .submit_3d(&command, ctx_id, None)
                        .map_err(|_| Error::new(Errno::EIO))?;
                }

                let ctx_id = if host3d_blob {
                    self.ensure_virtio_gpu_context(&virtio_gpu)?
                } else {
                    0
                };

                let backend = memfd_object_create("virtio-gpu-blob", user_data.size)?;
                let sg = self.virtio_gpu_sg_from_backend(&backend)?;

                let gem_obj = virtio_gpu_blob_object_create(
                    user_data.size,
                    Some(&sg),
                    backend,
                    user_data.blob_mem,
                    user_data.blob_flags,
                    user_data.blob_id,
                    ctx_id,
                    guest_blob,
                    host3d_blob,
                )
                .map_err(|_| Error::new(Errno::EINVAL))?;

                let handle = self.next_handle();
                self.insert_gem(handle, gem_obj.clone());

                let resource_id =
                    virtio_gpu_obj_resource_id(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;

                user_data.bo_handle = handle;
                user_data.res_handle = resource_id;

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuContextInit => {
                let user_data: aster_virtio::device::gpu::drm::VirtioGpuContextInit = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                if !virtio_gpu.has_context_init() || !virtio_gpu.has_virgl_3d() {
                    return_errno!(Errno::EINVAL);
                }

                if user_data.num_params > 4 {
                    return_errno!(Errno::EINVAL);
                }

                let params: Vec<aster_virtio::device::gpu::drm::VirtioGpuContextSetParam> =
                    self.read_user_array(user_data.ctx_set_params, user_data.num_params)?;

                let supported_capset_mask = self.virtio_gpu_supported_capset_mask(&virtio_gpu);
                let mut state = self.virtio_gpu_context.lock();
                if state.context_created {
                    return_errno!(Errno::EEXIST);
                }

                for param in params {
                    match param.param {
                        virtio_gpu_drm::VIRTGPU_CONTEXT_PARAM_CAPSET_ID => {
                            if param.value > VIRTGPU_MAX_CAPSET_ID {
                                return_errno!(Errno::EINVAL);
                            }
                            if (supported_capset_mask & (1u64 << param.value)) == 0 {
                                return_errno!(Errno::EINVAL);
                            }
                            if (state.context_init
                                & virtio_gpu_drm::VIRTIO_GPU_CONTEXT_INIT_CAPSET_ID_MASK)
                                != 0
                            {
                                return_errno!(Errno::EINVAL);
                            }

                            state.context_init |= (param.value as u32)
                                & virtio_gpu_drm::VIRTIO_GPU_CONTEXT_INIT_CAPSET_ID_MASK;
                        }
                        virtio_gpu_drm::VIRTGPU_CONTEXT_PARAM_NUM_RINGS => {
                            if state.rings_initialized {
                                return_errno!(Errno::EINVAL);
                            }

                            let num_rings = u32::try_from(param.value)
                                .map_err(|_| Error::new(Errno::EINVAL))?;
                            if num_rings > VIRTGPU_MAX_RINGS {
                                return_errno!(Errno::EINVAL);
                            }

                            state.num_rings = num_rings;
                            state.rings_initialized = true;
                        }
                        virtio_gpu_drm::VIRTGPU_CONTEXT_PARAM_POLL_RINGS_MASK => {
                            if state.ring_idx_mask != 0 {
                                return_errno!(Errno::EINVAL);
                            }
                            state.ring_idx_mask = param.value;
                        }
                        virtio_gpu_drm::VIRTGPU_CONTEXT_PARAM_DEBUG_NAME => {
                            if state.explicit_debug_name {
                                return_errno!(Errno::EINVAL);
                            }

                            let (debug_name, debug_name_len) =
                                self.read_user_cstring::<VIRTGPU_DEBUG_NAME_MAX_LEN>(param.value)?;
                            state.debug_name = debug_name;
                            state.debug_name_len = debug_name_len;
                            state.explicit_debug_name = true;
                        }
                        _ => return_errno!(Errno::EINVAL),
                    }
                }

                if state.ring_idx_mask != 0 {
                    let valid_ring_mask = if state.num_rings >= u64::BITS {
                        u64::MAX
                    } else if state.num_rings == 0 {
                        0
                    } else {
                        (1u64 << state.num_rings) - 1
                    };

                    if (state.ring_idx_mask & !valid_ring_mask) != 0 {
                        return_errno!(Errno::EINVAL);
                    }
                }

                let _ = self.ensure_virtio_gpu_context_locked(&virtio_gpu, &mut state)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuMap => {
                let mut user_data: aster_virtio::device::gpu::drm::VirtioGpuMap = cmd.read()?;

                user_data.offset = self.gem_map_offset(user_data.handle)?;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuResourceInfo => {
                let mut user_data: aster_virtio::device::gpu::drm::VirtioGpuResourceInfo = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };
                let _ = virtio_gpu;

                // Match Linux: lookup by bo_handle in this drm_file's GEM table.
                let gem_obj = self
                    .lookup_gem(&user_data.bo_handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;

                let size_u64 = gem_obj.size();
                user_data.size = u32::try_from(size_u64).map_err(|_| Error::new(Errno::EOVERFLOW))?;
                user_data.res_handle =
                    virtio_gpu_obj_resource_id(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;

                let (guest_blob, host3d_blob, blob_mem) =
                    virtio_gpu_blob_mem_by_gem(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;
                if host3d_blob || guest_blob {
                    user_data.blob_mem = blob_mem;
                }

                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuTransferFromHost => {
                let user_data: aster_virtio::device::gpu::drm::VirtioGpuTransferFromHost = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                if !virtio_gpu.has_virgl_3d() {
                    return_errno!(Errno::ENOSYS);
                }

                let gem_obj = self
                    .lookup_gem(&user_data.bo_handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;

                let (guest_blob, host3d_blob) =
                    virtio_gpu_blob_state_by_gem(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;
                if guest_blob && !host3d_blob {
                    return_errno!(Errno::EINVAL);
                }
                if !host3d_blob && (user_data.stride != 0 || user_data.layer_stride != 0) {
                    return_errno!(Errno::EINVAL);
                }

                let resource_id =
                    virtio_gpu_obj_resource_id(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;

                let ctx_id = self.ensure_virtio_gpu_context(&virtio_gpu)?;

                self.reset_gem_wait_point(&user_data.bo_handle);

                let transfer_box = aster_virtio::device::gpu::VirtioGpuBox {
                    x: user_data.box_.x,
                    y: user_data.box_.y,
                    z: user_data.box_.z,
                    w: user_data.box_.w,
                    h: user_data.box_.h,
                    d: user_data.box_.d,
                };

                let transfer_result = virtio_gpu
                    .transfer_from_host_3d(
                        ctx_id,
                        resource_id,
                        transfer_box,
                        user_data.level,
                        user_data.offset as u64,
                        user_data.stride,
                        user_data.layer_stride,
                    )
                    .map_err(|_| Error::new(Errno::EIO));

                self.signal_gem_wait_point(&user_data.bo_handle);

                transfer_result?;

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuTransferToHost => {
                let user_data: aster_virtio::device::gpu::drm::VirtioGpuTransferToHost = cmd.read()?;

                let Some(virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                let gem_obj = self
                    .lookup_gem(&user_data.bo_handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;

                let (guest_blob, host3d_blob) =
                    virtio_gpu_blob_state_by_gem(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;
                if guest_blob && !host3d_blob {
                    return_errno!(Errno::EINVAL);
                }

                let resource_id =
                    virtio_gpu_obj_resource_id(&gem_obj).map_err(|_| Error::new(Errno::EINVAL))?;

                self.reset_gem_wait_point(&user_data.bo_handle);

                if !virtio_gpu.has_virgl_3d() {
                    let rect = aster_virtio::device::gpu::VirtioGpuRect {
                        x: user_data.box_.x,
                        y: user_data.box_.y,
                        width: user_data.box_.w,
                        height: user_data.box_.h,
                    };

                    let transfer_result = virtio_gpu
                        .transfer_to_host_2d(resource_id, rect, user_data.offset as u64)
                        .map_err(|_| Error::new(Errno::EIO));
                    self.signal_gem_wait_point(&user_data.bo_handle);
                    transfer_result?;
                } else {
                    let ctx_id = self.ensure_virtio_gpu_context(&virtio_gpu)?;

                    if !host3d_blob && (user_data.stride != 0 || user_data.layer_stride != 0) {
                        self.signal_gem_wait_point(&user_data.bo_handle);
                        return_errno!(Errno::EINVAL);
                    }

                    let transfer_box = aster_virtio::device::gpu::VirtioGpuBox {
                        x: user_data.box_.x,
                        y: user_data.box_.y,
                        z: user_data.box_.z,
                        w: user_data.box_.w,
                        h: user_data.box_.h,
                        d: user_data.box_.d,
                    };

                    let transfer_result = virtio_gpu
                        .transfer_to_host_3d(
                            ctx_id,
                            resource_id,
                            transfer_box,
                            user_data.level,
                            user_data.offset as u64,
                            user_data.stride,
                            user_data.layer_stride,
                        )
                        .map_err(|_| Error::new(Errno::EIO));
                    self.signal_gem_wait_point(&user_data.bo_handle);
                    transfer_result?;
                }

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlVirtioGpuWait => {
                let user_data: aster_virtio::device::gpu::drm::VirtioGpuWait = cmd.read()?;

                let Some(_virtio_gpu) = self.virtio_gpu_device() else {
                    return_errno!(Errno::ENOTTY);
                };

                let _gem_obj = self
                    .lookup_gem(&user_data.handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;
                let point = self
                    .lookup_gem_wait_point(&user_data.handle)
                    .ok_or_else(|| Error::new(Errno::ENOENT))?;

                let signaled = if (user_data.flags & virtio_gpu_drm::VIRTGPU_WAIT_NOWAIT) != 0 {
                    point.is_signaled()
                } else {
                    self.gem_wait_common(&point, Some(Duration::from_secs(15)))?
                };

                if !signaled {
                    return_errno!(Errno::EBUSY);
                }

                cmd.write(&user_data)?;
                Ok(0)
            }
            _ => {
                let driver = self.device.driver();
                match driver.handle_command(raw_ioctl.cmd(), raw_ioctl.arg()) {
                    Ok(()) => Ok(0),
                    Err(err) => {
                        // TODO: handle error
                        match err {
                            DrmError::NotSupported | DrmError::NotFound => {
                                log::debug!(
                                    "the ioctl command {:#x} is unknown for drm devices",
                                    raw_ioctl.cmd()
                                );
                                return_errno_with_message!(
                                    Errno::ENOTTY,
                                    "the ioctl command is unknown"
                                );
                            }
                            _ => Err(err.into()),
                        }
                    }
                }
            }
        })
    }
}

impl Drop for DrmFile {
    fn drop(&mut self) {
        let Some(virtio_gpu) = self.virtio_gpu_device() else {
            return;
        };

        let state = self.virtio_gpu_context.lock();
        if state.context_created {
            let _ = virtio_gpu.context_destroy(state.ctx_id);
        }
    }
}
