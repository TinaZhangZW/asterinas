// SPDX-License-Identifier: MPL-2.0

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::{
    fmt::Debug,
    hint::spin_loop,
    mem::size_of,
    sync::atomic::{AtomicU32, Ordering},
};

use aster_gpu::{GpuDevice, GpuFramebufferInfo, GpuPixelFormat};
use log::{debug, warn};
use ostd::{
    Pod,
    arch::trap::TrapFrame,
    mm::{HasDaddr, HasPaddr, HasSize, VmIo, PAGE_SIZE, dma::DmaStream},
    sync::SpinLock,
};
use spin::Once;

use super::{
    VIRTIO_GPU_DRIVER_NAME,
    config::{VirtioGpuConfig, VirtioGpuFeatures},
    drm, sysfs,
    virtgpu::{
        DrmVirtgpuContextInit, DrmVirtgpuExecbuffer, DrmVirtgpuGetCaps, DrmVirtgpuGetparam,
        DrmVirtgpuMap, DrmVirtgpuResourceCreate, DrmVirtgpuResourceCreateBlob,
        DrmVirtgpuResourceInfo, DrmVirtgpuResourceUnref, DrmVirtgpuTransfer, DrmVirtgpuWait,
        VirtgpuParam, VIRTGPU_CONTEXT_INIT, VIRTGPU_EXECBUFFER, VIRTGPU_GET_CAPS, VIRTGPU_GETPARAM,
        VIRTGPU_MAP, VIRTGPU_RESOURCE_CREATE, VIRTGPU_RESOURCE_CREATE_BLOB, VIRTGPU_RESOURCE_INFO,
        VIRTGPU_RESOURCE_UNREF, VIRTGPU_TRANSFER_FROM_HOST, VIRTGPU_TRANSFER_TO_HOST, VIRTGPU_WAIT,
    },
};
use crate::{
    device::VirtioDeviceError,
    queue::{QueueError, VirtQueue},
    transport::{ConfigManager, VirtioTransport},
};
use aster_util::mem_obj_slice::Slice;

const QUEUE_CONTROL: u16 = 0;
const QUEUE_CURSOR: u16 = 1;
const QUEUE_SIZE: u16 = 64;

static VIRTIO_GPU_DRIVER_ONCE: Once<()> = Once::new();
static VIRTIO_GPU_DEVICES: Once<SpinLock<Vec<Arc<VirtioGpuDevice>>>> = Once::new();

const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
#[expect(dead_code)]
const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;

const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
#[expect(dead_code)]
const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;

/// Virtio GPU device implementation.
pub struct VirtioGpuDevice {
    config_manager: ConfigManager<VirtioGpuConfig>,
    transport: SpinLock<Box<dyn VirtioTransport>>,
    control_queue: SpinLock<VirtQueue>,
    cursor_queue: SpinLock<VirtQueue>,
    framebuffer: SpinLock<Option<Framebuffer>>,
    resources: SpinLock<BTreeMap<u32, VirtioGpuResource>>,
    next_resource_id: AtomicU32,
}

impl Debug for VirtioGpuDevice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtioGpuDevice")
            .field("config", &self.config_manager.read_config())
            .field("control_queue", &self.control_queue)
            .field("cursor_queue", &self.cursor_queue)
            .finish()
    }
}

impl GpuDevice for VirtioGpuDevice {
    fn driver_name(&self) -> &str {
        VIRTIO_GPU_DRIVER_NAME
    }

    fn framebuffer_info(&self) -> Option<GpuFramebufferInfo> {
        self.framebuffer_info_snapshot()
    }
}

impl VirtioGpuDevice {
    /// Negotiate features for device-specified bits (0..=23, 50..=63).
    pub(crate) fn negotiate_features(features: u64) -> u64 {
        let device_features = VirtioGpuFeatures::from_bits_truncate(features);
        if !device_features.is_empty() {
            debug!("virtio-gpu unsupported features: {:?}", device_features);
        }
        0
    }

    /// Initialize a virtio GPU device and register it with the GPU subsystem.
    pub fn init(mut transport: Box<dyn VirtioTransport>) -> Result<(), VirtioDeviceError> {
        let num_queues = transport.num_queues();
        if num_queues < 2 {
            return Err(VirtioDeviceError::QueuesAmountDoNotMatch(num_queues, 2));
        }

        let config_manager = VirtioGpuConfig::new_manager(transport.as_ref());
        debug!("virtio_gpu_config = {:?}", config_manager.read_config());

        let control_queue = VirtQueue::new(QUEUE_CONTROL, QUEUE_SIZE, transport.as_mut())
            .expect("create control queue failed");
        let cursor_queue = VirtQueue::new(QUEUE_CURSOR, QUEUE_SIZE, transport.as_mut())
            .expect("create cursor queue failed");

        let device = Arc::new(Self {
            config_manager,
            transport: SpinLock::new(transport),
            control_queue: SpinLock::new(control_queue),
            cursor_queue: SpinLock::new(cursor_queue),
            framebuffer: SpinLock::new(None),
            resources: SpinLock::new(BTreeMap::new()),
            next_resource_id: AtomicU32::new(1),
        });

        {
            let mut transport = device.transport.disable_irq().lock();
            transport
                .register_cfg_callback(Box::new(config_space_change))
                .unwrap();
            transport.finish_init();
        }

        let devices = VIRTIO_GPU_DEVICES.call_once(|| SpinLock::new(Vec::new()));
        let virtio_index = {
            let mut guard = devices.disable_irq().lock();
            let index = guard.len() as u32;
            guard.push(device.clone());
            index
        };

        VIRTIO_GPU_DRIVER_ONCE.call_once(|| {
            drm::register();
        });

        aster_gpu::register_device(device).expect("failed to register virtio-gpu GpuDevice");

        if let Err(err) = sysfs::register_virtio_gpu_sysfs(virtio_index, virtio_index) {
            warn!("virtio-gpu sysfs registration failed: {:?}", err);
        }
        Ok(())
    }

    pub(super) fn device_by_index(index: u32) -> Option<Arc<VirtioGpuDevice>> {
        let devices = VIRTIO_GPU_DEVICES.get()?;
        devices.lock().get(index as usize).cloned()
    }

    fn submit_control<R: Pod, S: Pod>(&self, req: &R) -> Result<S, VirtioDeviceError> {
        let req_stream = Arc::new(DmaStream::alloc(1, false).unwrap());
        let resp_stream = Arc::new(DmaStream::alloc(1, false).unwrap());

        let req_slice = Slice::new(&req_stream, 0..size_of::<R>());
        req_slice.write_val(0, req).unwrap();
        req_slice.sync_to_device().unwrap();

        let resp_slice = Slice::new(&resp_stream, 0..size_of::<S>());
        let resp_init = S::new_uninit();
        resp_slice.write_val(0, &resp_init).unwrap();
        resp_slice.sync_to_device().unwrap();

        let token = {
            let mut queue = self.control_queue.disable_irq().lock();
            let token = queue
                .add_dma_buf(&[&req_slice], &[&resp_slice])
                .map_err(|_| VirtioDeviceError::QueueUnknownError)?;
            if queue.should_notify() {
                queue.notify();
            }
            token
        };

        loop {
            let mut queue = self.control_queue.disable_irq().lock();
            match queue.pop_used_with_token(token) {
                Ok(_) => break,
                Err(QueueError::NotReady) => {
                    drop(queue);
                    spin_loop();
                }
                Err(_) => return Err(VirtioDeviceError::QueueUnknownError),
            }
        }

        resp_slice.sync_from_device().unwrap();
        Ok(resp_slice.read_val(0).unwrap())
    }

    fn submit_control_ok<R: Pod>(&self, req: &R) -> Result<(), VirtioDeviceError> {
        let resp: VirtioGpuCtrlHdr = self.submit_control(req)?;
        if resp.type_ != VIRTIO_GPU_RESP_OK_NODATA {
            return Err(VirtioDeviceError::QueueUnknownError);
        }
        Ok(())
    }

    fn query_display_info(&self) -> Result<VirtioGpuRespDisplayInfo, VirtioDeviceError> {
        let req = VirtioGpuGetDisplayInfo {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_GET_DISPLAY_INFO),
        };
        let resp: VirtioGpuRespDisplayInfo = self.submit_control(&req)?;
        if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
            return Err(VirtioDeviceError::QueueUnknownError);
        }
        Ok(resp)
    }

    pub(super) fn preferred_scanout(
        &self,
    ) -> Result<Option<(u32, VirtioGpuRect)>, VirtioDeviceError> {
        let cfg = self.config_manager.read_config();
        let scanout_limit = cfg.num_scanouts.min(VIRTIO_GPU_MAX_SCANOUTS as u32);
        let resp = self.query_display_info()?;
        for (index, info) in resp.pmodes.iter().take(scanout_limit as usize).enumerate() {
            if info.enabled != 0 && info.r.width > 0 && info.r.height > 0 {
                return Ok(Some((index as u32, info.r)));
            }
        }
        Ok(None)
    }

    pub(super) fn setup_scanout(
        &self,
        scanout_id: u32,
        rect: VirtioGpuRect,
    ) -> Result<(), VirtioDeviceError> {
        let width = rect.width.max(1);
        let height = rect.height.max(1);
        let stride = width * 4;
        let fb_size = stride as usize * height as usize;
        let pages = fb_size.div_ceil(PAGE_SIZE);
        let buffer = Arc::new(DmaStream::alloc(pages, false).unwrap());
        let resource_id = self.next_resource_id.fetch_add(1, Ordering::Relaxed);

        let create = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_CREATE_2D),
            resource_id,
            format: VirtioGpuFormat::B8G8R8X8Unorm as u32,
            width,
            height,
        };
        self.submit_control_ok(&create)?;

        let attach = VirtioGpuResourceAttachBacking {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id,
            nr_entries: 1,
            entries: [VirtioGpuMemEntry {
                addr: buffer.daddr() as u64,
                length: fb_size as u32,
                _padding: 0,
            }],
        };
        self.submit_control_ok(&attach)?;

        let scanout = VirtioGpuSetScanout {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_SET_SCANOUT),
            r: rect,
            scanout_id,
            resource_id,
        };
        self.submit_control_ok(&scanout)?;

        let mut fb = self.framebuffer.lock();
        *fb = Some(Framebuffer {
            resource_id,
            width,
            height,
            stride,
            buffer,
        });
        Ok(())
    }

    pub(super) fn framebuffer_info_snapshot(&self) -> Option<GpuFramebufferInfo> {
        let fb = self.framebuffer.lock();
        let fb = fb.as_ref()?;
        Some(GpuFramebufferInfo {
            paddr: fb.buffer.paddr(),
            size: fb.buffer.size(),
            width: fb.width as usize,
            height: fb.height as usize,
            line_size: fb.stride as usize,
            pixel_format: GpuPixelFormat::BgrReserved,
        })
    }

    pub(super) fn ioctl(&self, cmd: u32, data: &mut [u8]) -> Result<(), ()> {
        match cmd {
            VIRTGPU_MAP => self.handle_map(data),
            VIRTGPU_EXECBUFFER => self.handle_execbuffer(data),
            VIRTGPU_GETPARAM => self.handle_getparam(data),
            VIRTGPU_RESOURCE_CREATE => self.handle_resource_create(data),
            VIRTGPU_RESOURCE_INFO => self.handle_resource_info(data),
            VIRTGPU_RESOURCE_UNREF => self.handle_resource_unref(data),
            VIRTGPU_TRANSFER_FROM_HOST => self.handle_transfer_from_host(data),
            VIRTGPU_TRANSFER_TO_HOST => self.handle_transfer_to_host(data),
            VIRTGPU_WAIT => self.handle_wait(data),
            VIRTGPU_GET_CAPS => self.handle_get_caps(data),
            VIRTGPU_RESOURCE_CREATE_BLOB => self.handle_resource_create_blob(data),
            VIRTGPU_CONTEXT_INIT => self.handle_context_init(data),
            _ => Err(()),
        }
    }

    fn handle_map(&self, data: &mut [u8]) -> Result<(), ()> {
        let mut req: DrmVirtgpuMap = read_pod(data)?;
        let resources = self.resources.lock();
        let resource_id = req.handle;
        if !resources.contains_key(&resource_id) {
            return Err(());
        }
        // No userspace mapping backend available yet; return a zero offset.
        req.offset = 0;
        write_pod(data, &req)
    }

    fn handle_execbuffer(&self, data: &mut [u8]) -> Result<(), ()> {
        let _req: DrmVirtgpuExecbuffer = read_pod(data)?;
        // 3D execution is not supported yet; treat as a no-op.
        Ok(())
    }

    fn handle_getparam(&self, data: &mut [u8]) -> Result<(), ()> {
        let mut req: DrmVirtgpuGetparam = read_pod(data)?;
        let value = match req.param {
            x if x == VirtgpuParam::Param3dFeatures as u64 => 0,
            x if x == VirtgpuParam::CapsetMaxId as u64 => 0,
            x if x == VirtgpuParam::CapsetMaxVersion as u64 => 0,
            x if x == VirtgpuParam::CapsetMaxSize as u64 => 0,
            x if x == VirtgpuParam::ContextInit as u64 => 0,
            _ => 0,
        };
        req.value = value;
        write_pod(data, &req)
    }

    fn handle_resource_create(&self, data: &mut [u8]) -> Result<(), ()> {
        let mut req: DrmVirtgpuResourceCreate = read_pod(data)?;
        let width = req.width.max(1);
        let height = req.height.max(1);
        let stride = width.saturating_mul(4);
        let size = if req.size != 0 {
            req.size
        } else {
            (stride as u64).saturating_mul(height as u64)
        };

        let resource_id = self.next_resource_id.fetch_add(1, Ordering::Relaxed);
        let buffer = alloc_dma_buffer(size)?;

        let create = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_CREATE_2D),
            resource_id,
            format: VirtioGpuFormat::B8G8R8X8Unorm as u32,
            width,
            height,
        };
        self.submit_control_ok(&create).map_err(|_| ())?;

        let attach = VirtioGpuResourceAttachBacking {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id,
            nr_entries: 1,
            entries: [VirtioGpuMemEntry {
                addr: buffer.daddr() as u64,
                length: size as u32,
                _padding: 0,
            }],
        };
        self.submit_control_ok(&attach).map_err(|_| ())?;

        let resource = VirtioGpuResource {
            resource_id,
            width,
            height,
            stride,
            size,
            _buffer: buffer,
        };
        self.resources.lock().insert(resource_id, resource);

        req.res_handle = resource_id;
        req.bo_handle = resource_id;
        req.size = size;
        write_pod(data, &req)
    }

    fn handle_resource_info(&self, data: &mut [u8]) -> Result<(), ()> {
        let mut req: DrmVirtgpuResourceInfo = read_pod(data)?;
        let resource_id = if req.res_handle != 0 {
            req.res_handle
        } else {
            req.bo_handle
        };
        let resources = self.resources.lock();
        let resource = resources.get(&resource_id).ok_or(())?;
        req.res_handle = resource.resource_id;
        req.bo_handle = resource.resource_id;
        req.size = resource.size.min(u32::MAX as u64) as u32;
        req.stride = resource.stride;
        write_pod(data, &req)
    }

    fn handle_resource_unref(&self, data: &mut [u8]) -> Result<(), ()> {
        let req: DrmVirtgpuResourceUnref = read_pod(data)?;
        let resource_id = req.res_handle;
        let resource = self.resources.lock().remove(&resource_id).ok_or(())?;

        let unref = VirtioGpuResourceUnref {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_UNREF),
            resource_id: resource.resource_id,
            _padding: 0,
        };
        self.submit_control_ok(&unref).map_err(|_| ())
    }

    fn handle_transfer_to_host(&self, data: &mut [u8]) -> Result<(), ()> {
        let req: DrmVirtgpuTransfer = read_pod(data)?;
        let resource_id = if req.res_handle != 0 {
            req.res_handle
        } else {
            req.bo_handle
        };
        let resources = self.resources.lock();
        let resource = resources.get(&resource_id).ok_or(())?;

        let rect = VirtioGpuRect {
            x: req.box_.x,
            y: req.box_.y,
            width: req.box_.w.min(resource.width),
            height: req.box_.h.min(resource.height),
        };

        let transfer = VirtioGpuTransferToHost2d {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
            r: rect,
            offset: req.offset,
            resource_id: resource.resource_id,
            _padding: 0,
        };
        self.submit_control_ok(&transfer).map_err(|_| ())?;

        let flush = VirtioGpuResourceFlush {
            hdr: VirtioGpuCtrlHdr::new(VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: rect,
            resource_id: resource.resource_id,
            _padding: 0,
        };
        self.submit_control_ok(&flush).map_err(|_| ())?;
        Ok(())
    }

    fn handle_transfer_from_host(&self, data: &mut [u8]) -> Result<(), ()> {
        let _req: DrmVirtgpuTransfer = read_pod(data)?;
        // Not supported by the 2D path; ignore for now.
        Ok(())
    }

    fn handle_wait(&self, data: &mut [u8]) -> Result<(), ()> {
        let _req: DrmVirtgpuWait = read_pod(data)?;
        // No explicit fence support; treat as immediate completion.
        Ok(())
    }

    fn handle_get_caps(&self, data: &mut [u8]) -> Result<(), ()> {
        let mut req: DrmVirtgpuGetCaps = read_pod(data)?;
        req.size = 0;
        write_pod(data, &req)
    }

    fn handle_resource_create_blob(&self, data: &mut [u8]) -> Result<(), ()> {
        let _req: DrmVirtgpuResourceCreateBlob = read_pod(data)?;
        // Blob resources are not supported yet.
        Err(())
    }

    fn handle_context_init(&self, data: &mut [u8]) -> Result<(), ()> {
        let _req: DrmVirtgpuContextInit = read_pod(data)?;
        // 3D contexts are not supported yet.
        Ok(())
    }
}

fn config_space_change(_: &TrapFrame) {
    debug!("virtio-gpu device configuration space change");
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuCtrlHdr {
    type_: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    padding: u32,
}

impl VirtioGpuCtrlHdr {
    fn new(type_: u32) -> Self {
        Self {
            type_,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
pub(super) struct VirtioGpuRect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuDisplayOne {
    r: VirtioGpuRect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod)]
struct VirtioGpuRespDisplayInfo {
    hdr: VirtioGpuCtrlHdr,
    pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuGetDisplayInfo {
    hdr: VirtioGpuCtrlHdr,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuResourceCreate2d {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuResourceUnref {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    _padding: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuSetScanout {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuResourceFlush {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    resource_id: u32,
    _padding: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuTransferToHost2d {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    offset: u64,
    resource_id: u32,
    _padding: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuMemEntry {
    addr: u64,
    length: u32,
    _padding: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
struct VirtioGpuResourceAttachBacking {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    entries: [VirtioGpuMemEntry; 1],
}

#[derive(Debug)]
struct VirtioGpuResource {
    resource_id: u32,
    width: u32,
    height: u32,
    stride: u32,
    size: u64,
    _buffer: Arc<DmaStream>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Default)]
#[expect(dead_code)]
struct VirtioGpuResourceDetachBacking {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    _padding: u32,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone)]
enum VirtioGpuFormat {
    B8G8R8X8Unorm = 2,
}

#[derive(Debug)]
#[expect(dead_code)]
struct Framebuffer {
    resource_id: u32,
    width: u32,
    height: u32,
    stride: u32,
    buffer: Arc<DmaStream>,
}

fn alloc_dma_buffer(size: u64) -> Result<Arc<DmaStream>, ()> {
    let pages = (size as usize).div_ceil(PAGE_SIZE);
    let buffer = DmaStream::alloc(pages, false).map_err(|_| ())?;
    Ok(Arc::new(buffer))
}

fn read_pod<T: Pod>(data: &[u8]) -> Result<T, ()> {
    if data.len() < size_of::<T>() {
        return Err(());
    }
    let mut val = T::new_zeroed();
    val.as_bytes_mut()[..size_of::<T>()].copy_from_slice(&data[..size_of::<T>()]);
    Ok(val)
}

fn write_pod<T: Pod>(data: &mut [u8], val: &T) -> Result<(), ()> {
    if data.len() < size_of::<T>() {
        return Err(());
    }
    data[..size_of::<T>()].copy_from_slice(&val.as_bytes()[..size_of::<T>()]);
    Ok(())
}
