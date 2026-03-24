use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use alloc::vec;

use aster_gpu::{GpuDevice, drm::{DrmError, gem::DrmGemObject, ioctl::DrmModeCreateDumb}};
use aster_gpu::drm::gem::DrmGemBackend;
use crate::device::gpu::{
    VirtioGpuCtrlHdr,
    VirtioGpuMemEntry,
};
use ostd::sync::SpinLock;
use spin::Once;

use crate::device::gpu::{DEVICE_NAME, device::VirtioGpuDevice};

pub struct VirtioGpuObject {
    gem_object: Arc<DrmGemObject>,
    hw_res_handle: u32,
    width: u32,
    height: u32,
    pitch: u32,
    size: u64,
    dumb: bool,
    created: bool,
    // hdr returned by the last control command (attach/create). Some host
    // implementations may communicate additional info here.
    attach_hdr: Option<VirtioGpuCtrlHdr>,
    host3d_blob: bool,
    guest_blob: bool,
    blob_mem: u32,
    blob_flags: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioGpuObjectParams {
    pub virgl: bool,
    pub blob: bool,
    pub target: u32,
    pub format: u32,
    pub bind: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
    pub blob_mem: u32,
    pub blob_flags: u32,
    pub blob_id: u64,
    pub ctx_id: u32,
    pub guest_blob: bool,
    pub host3d_blob: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct VirtioGpuSgEntry {
    pub addr: u64,
    pub len: u32,
}

#[derive(Debug, Clone, Default)]
pub struct VirtioGpuSgTable {
    pub entries: Vec<VirtioGpuSgEntry>,
}

impl VirtioGpuObject {
    fn new(
        gem_object: Arc<DrmGemObject>,
        hw_res_handle: u32,
        width: u32,
        height: u32,
        pitch: u32,
        size: u64,
        attach_hdr: VirtioGpuCtrlHdr,
    ) -> Self {
        Self {
            gem_object,
            hw_res_handle,
            width,
            height,
            pitch,
            size,
            dumb: true,
            created: true,
            attach_hdr: Some(attach_hdr),
            host3d_blob: false,
            guest_blob: false,
            blob_mem: 0,
            blob_flags: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct VirtioGpuCreateResult {
    resource_id: u32,
    create_hdr: VirtioGpuCtrlHdr,
}

static VIRTIO_GPU_OBJECTS: Once<SpinLock<BTreeMap<usize, VirtioGpuObject>>> = Once::new();

fn object_key(gem: &Arc<DrmGemObject>) -> usize {
    Arc::as_ptr(gem) as usize
}

fn objects_map() -> &'static SpinLock<BTreeMap<usize, VirtioGpuObject>> {
    VIRTIO_GPU_OBJECTS.call_once(|| SpinLock::new(BTreeMap::new()))
}

/// Return the hardware resource handle associated with the given GEM object.
/// This function assumes the object was previously created by
/// `virtio_gpu_object_create` and therefore has an attached backing.
pub fn virtio_gpu_obj_resource_id(
    gem_object: &Arc<DrmGemObject>,
) -> Result<u32, DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .get(&object_key(gem_object))
        .ok_or(DrmError::Invalid)?;
    Ok(obj.hw_res_handle)
}

pub fn virtio_gpu_object_unref(gem_object: &Arc<DrmGemObject>) -> Result<(), DrmError> {
    let key = object_key(gem_object);
    let hw_res = {
        let mut objs = objects_map().lock();
        let obj = objs.remove(&key).ok_or(DrmError::Invalid)?;
        obj.hw_res_handle
    };

    with_vgpu(|vgpu| {
        // Best-effort detach before unref; unref is authoritative for freeing resource.
        let _ = vgpu.resource_detach_backing(hw_res);
        vgpu.resource_unref(hw_res).map_err(|_| DrmError::Invalid)?;
        Ok(())
    })
}

fn with_vgpu<R>(f: impl FnOnce(&VirtioGpuDevice) -> Result<R, DrmError>) -> Result<R, DrmError> {
    for dev in aster_gpu::registered_devices() {
        if dev.driver_name() != DEVICE_NAME {
            continue;
        }
        if let Some(vgpu) = (dev.as_ref() as &dyn GpuDevice).downcast_ref::<VirtioGpuDevice>() {
            return f(vgpu);
        }
    }
    Err(DrmError::Invalid)
}

pub fn virtio_gpu_object_create(
    size: u64,
    pitch: u32,
    width: u32,
    height: u32,
    initial_sg: Option<&VirtioGpuSgTable>,
    backend: Arc<dyn DrmGemBackend>,
    params: &VirtioGpuObjectParams,
) -> Result<Arc<DrmGemObject>, DrmError> {
    virtio_gpu_object_create_with_backend(size, pitch, width, height, initial_sg, backend, params)
}

pub fn virtio_gpu_blob_object_create(
    size: u64,
    initial_sg: Option<&VirtioGpuSgTable>,
    backend: Arc<dyn DrmGemBackend>,
    blob_mem: u32,
    blob_flags: u32,
    blob_id: u64,
    ctx_id: u32,
    guest_blob: bool,
    host3d_blob: bool,
) -> Result<Arc<DrmGemObject>, DrmError> {
    let params = VirtioGpuObjectParams {
        blob: true,
        blob_mem,
        blob_flags,
        blob_id,
        ctx_id,
        guest_blob,
        host3d_blob,
        ..Default::default()
    };

    virtio_gpu_object_create_with_backend(size, 0, 0, 0, initial_sg, backend, &params)
}

pub fn virtio_gpu_object_create_with_backend(
    size: u64,
    pitch: u32,
    width: u32,
    height: u32,
    initial_sg: Option<&VirtioGpuSgTable>,
    backend: Arc<dyn DrmGemBackend>,
    params: &VirtioGpuObjectParams,
) -> Result<Arc<DrmGemObject>, DrmError> {
    let gem_object = Arc::new(DrmGemObject::new(size, pitch, backend));

    let create_res = with_vgpu(|vgpu| {
        let resource_id = vgpu.alloc_resource_id();
        if params.blob {
            let entries: Vec<VirtioGpuMemEntry> = initial_sg
                .map(|sg| {
                    sg.entries
                        .iter()
                        .map(|entry| VirtioGpuMemEntry {
                            addr: entry.addr,
                            length: entry.len,
                            padding: 0,
                        })
                        .collect()
                })
                .unwrap_or_default();

            let create_hdr = vgpu
                .resource_create_blob_with_fence(
                    resource_id,
                    params.blob_mem,
                    params.blob_flags,
                    params.blob_id,
                    size,
                    params.ctx_id,
                    entries.as_slice(),
                )
                .map_err(|_| DrmError::Invalid)?;
            Ok(VirtioGpuCreateResult {
                resource_id,
                create_hdr,
            })
        } else if params.virgl {
            let create_hdr = vgpu
                .resource_create_3d_with_fence(
                    resource_id,
                    params.target,
                    params.format,
                    params.bind,
                    params.width,
                    params.height,
                    params.depth,
                    params.array_size,
                    params.last_level,
                    params.nr_samples,
                    params.flags,
                )
                .map_err(|_| DrmError::Invalid)?;
            Ok(VirtioGpuCreateResult {
                resource_id,
                create_hdr,
            })
        } else {
            let create_hdr = vgpu
                .resource_create_2d_with_fence(
                    resource_id,
                    params.format,
                    width,
                    height,
                )
                .map_err(|_| DrmError::Invalid)?;
            Ok(VirtioGpuCreateResult {
                resource_id,
                create_hdr,
            })
        }
    })?;

    let mut virtio_gpu_obj = VirtioGpuObject::new(
        gem_object.clone(),
        create_res.resource_id,
        width,
        height,
        pitch,
        size,
        create_res.create_hdr,
    );
    virtio_gpu_obj.dumb = !(params.virgl || params.blob);
    virtio_gpu_obj.guest_blob = params.guest_blob;
    virtio_gpu_obj.host3d_blob = params.host3d_blob;
    virtio_gpu_obj.blob_mem = params.blob_mem;
    virtio_gpu_obj.blob_flags = params.blob_flags;

    objects_map()
        .lock()
        .insert(object_key(&gem_object), virtio_gpu_obj);

    // Optional eager backing attach supplied by caller.
    // Blob resources pass SG entries in create_blob directly and do not use ATTACH_BACKING.
    if !params.blob {
        if let Some(sg) = initial_sg {
            virtio_gpu_attach_backing_sg(&gem_object, sg)?;
        }
    }

    Ok(gem_object)
}

/// Return metadata for a virtio-gpu resource identified by the host resource id.
pub fn virtio_gpu_resource_info_by_hw_res(hw_res: u32) -> Result<(u32, u32, u32, u64), DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .values()
        .find(|o| o.hw_res_handle == hw_res)
        .ok_or(DrmError::Invalid)?;

    Ok((obj.width, obj.height, obj.pitch, obj.size))
}

/// Return metadata for a virtio-gpu resource associated with a GEM object.
pub fn virtio_gpu_resource_info_by_gem(
    gem_object: &Arc<DrmGemObject>,
) -> Result<(u32, u32, u32, u64, u32), DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .get(&object_key(gem_object))
        .ok_or(DrmError::Invalid)?;

    Ok((
        obj.width,
        obj.height,
        obj.pitch,
        obj.size,
        obj.hw_res_handle,
    ))
}

/// Return blob flags associated with a virtio-gpu GEM object.
pub fn virtio_gpu_blob_state_by_gem(gem_object: &Arc<DrmGemObject>) -> Result<(bool, bool), DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .get(&object_key(gem_object))
        .ok_or(DrmError::Invalid)?;

    Ok((obj.guest_blob, obj.host3d_blob))
}

/// Return blob memory domain metadata for a virtio-gpu GEM object.
pub fn virtio_gpu_blob_mem_by_gem(gem_object: &Arc<DrmGemObject>) -> Result<(bool, bool, u32), DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .get(&object_key(gem_object))
        .ok_or(DrmError::Invalid)?;

    Ok((obj.guest_blob, obj.host3d_blob, obj.blob_mem))
}

/// Return the last control header associated with object creation.
/// The header carries the virtio fence id used by the creation command.
pub fn virtio_gpu_create_hdr_by_gem(gem_object: &Arc<DrmGemObject>) -> Result<VirtioGpuCtrlHdr, DrmError> {
    let objs = objects_map().lock();
    let obj = objs
        .get(&object_key(gem_object))
        .ok_or(DrmError::Invalid)?;
    obj.attach_hdr.ok_or(DrmError::Invalid)
}

/// Attach backing pages to an existing virtio-gpu resource.
///
/// The caller (usually file.rs) is responsible for flushing caches and
/// providing the physical address/size pairs.  This keeps filesystem
/// knowledge out of `gem.rs`.
pub(crate) fn virtio_gpu_attach_backing(
    gem_object: &Arc<DrmGemObject>,
    addr: u64,
    size: u32,
) -> Result<(), DrmError> {
    let sg = VirtioGpuSgTable {
        entries: vec![VirtioGpuSgEntry { addr, len: size }],
    };
    virtio_gpu_attach_backing_sg(gem_object, &sg)
}

pub(crate) fn virtio_gpu_attach_backing_sg(
    gem_object: &Arc<DrmGemObject>,
    sg_table: &VirtioGpuSgTable,
) -> Result<(), DrmError> {
    if sg_table.entries.is_empty() {
        return Err(DrmError::Invalid);
    }

    let hw_res = virtio_gpu_obj_resource_id(gem_object)?;
    let entries: Vec<VirtioGpuMemEntry> = sg_table
        .entries
        .iter()
        .map(|e| VirtioGpuMemEntry {
            addr: e.addr,
            length: e.len,
            padding: 0,
        })
        .collect();

    with_vgpu(|vgpu| {
        vgpu
            .resource_attach_backing_sg(hw_res, entries.as_slice())
            .map_err(|_| DrmError::Invalid)?;
        Ok(())
    })
}

pub fn virtio_gpu_mode_dumb_create_with_sg(
    args: &mut DrmModeCreateDumb,
    backend: Arc<dyn DrmGemBackend>,
    initial_sg: Option<&VirtioGpuSgTable>,
) -> Result<Arc<DrmGemObject>, DrmError> {
    if args.bpp != 32 {
        return Err(DrmError::Invalid);
    }

    let pitch = args.width.checked_mul(4).ok_or(DrmError::Invalid)?;
    let size_u32 = pitch.checked_mul(args.height).ok_or(DrmError::Invalid)?;
    let size = size_u32 as u64;

    args.pitch = pitch;
    args.size = size;

    let params = VirtioGpuObjectParams {
        format: crate::device::gpu::VirtioGpuFormat::B8G8R8X8Unorm as u32,
        width: args.width,
        height: args.height,
        ..Default::default()
    };

    virtio_gpu_object_create_with_backend(size, pitch, args.width, args.height, initial_sg, backend, &params)
}

pub fn virtio_gpu_mode_dumb_create_unreachable(
    _args: &mut DrmModeCreateDumb,
) -> Result<Arc<DrmGemObject>, DrmError> {
    // Dumb-create for virtio-gpu is handled in kernel/src/device/drm/file.rs via
    // `virtio_gpu_mode_dumb_create_with_sg`, where memfd is preallocated and SG is prepared.
    Err(DrmError::NotSupported)
}
