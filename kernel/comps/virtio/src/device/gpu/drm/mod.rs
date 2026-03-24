use alloc::{boxed::Box, sync::Arc};

use aster_gpu::{
    GpuDevice,
    drm::{
        DrmError,
        device::DrmDevice,
        driver::{DrmDriver, DrmDriverFeatures, DrmDriverOps, DumbCreateProvider},
        mode_config::{
            DrmModeConfig,
            framebuffer::{DrmFramebuffer, helper::drm_gem_fb_create_with_dirty},
            funcs::{ModeConfigFuncs, drm_atomic_helper_commit},
        },
    },
};
use ostd::Pod;

use crate::device::gpu::{
    device::VirtioGpuDevice,
    drm::{gem::virtio_gpu_mode_dumb_create_unreachable, output::virtio_gpu_output_init},
};

pub(crate) const DRIVER_NAME: &'static str = "virtio_gpu";
pub(crate) const DRIVER_DESC: &'static str = "virtio GPU";
pub(crate) const DRIVER_DATE: &'static str = "2026-01-02";

pub const DRM_VIRTGPU_GETPARAM: u8 = 0x03;
pub const DRM_VIRTGPU_RESOURCE_CREATE: u8 = 0x04;
pub const DRM_VIRTGPU_RESOURCE_INFO: u8 = 0x05;
pub const DRM_VIRTGPU_TRANSFER_FROM_HOST: u8 = 0x06;
pub const DRM_VIRTGPU_TRANSFER_TO_HOST: u8 = 0x07;
pub const DRM_VIRTGPU_WAIT: u8 = 0x08;
pub const DRM_VIRTGPU_GET_CAPS: u8 = 0x09;
pub const DRM_VIRTGPU_RESOURCE_CREATE_BLOB: u8 = 0x0a;
pub const DRM_VIRTGPU_CONTEXT_INIT: u8 = 0x0b;
pub const DRM_VIRTGPU_MAP: u8 = 0x01;
pub const DRM_VIRTGPU_EXECBUFFER: u8 = 0x02;

pub const VIRTGPU_EXECBUF_FENCE_FD_IN: u32 = 0x01;
pub const VIRTGPU_EXECBUF_FENCE_FD_OUT: u32 = 0x02;
pub const VIRTGPU_EXECBUF_RING_IDX: u32 = 0x04;
pub const VIRTGPU_EXECBUF_FLAGS: u32 =
    VIRTGPU_EXECBUF_FENCE_FD_IN | VIRTGPU_EXECBUF_FENCE_FD_OUT | VIRTGPU_EXECBUF_RING_IDX;

pub const VIRTGPU_EXECBUF_SYNCOBJ_RESET: u32 = 0x01;
pub const VIRTGPU_EXECBUF_SYNCOBJ_FLAGS: u32 = VIRTGPU_EXECBUF_SYNCOBJ_RESET;
pub const VIRTGPU_WAIT_NOWAIT: u32 = 0x01;
pub const VIRTGPU_BLOB_MEM_GUEST: u32 = 0x0001;
pub const VIRTGPU_BLOB_MEM_HOST3D: u32 = 0x0002;
pub const VIRTGPU_BLOB_MEM_HOST3D_GUEST: u32 = 0x0003;

pub const VIRTGPU_BLOB_FLAG_USE_MAPPABLE: u32 = 0x0001;
pub const VIRTGPU_BLOB_FLAG_USE_SHAREABLE: u32 = 0x0002;
pub const VIRTGPU_BLOB_FLAG_USE_CROSS_DEVICE: u32 = 0x0004;
pub const VIRTGPU_BLOB_FLAG_USE_MASK: u32 = VIRTGPU_BLOB_FLAG_USE_MAPPABLE
    | VIRTGPU_BLOB_FLAG_USE_SHAREABLE
    | VIRTGPU_BLOB_FLAG_USE_CROSS_DEVICE;

pub const VIRTGPU_PARAM_3D_FEATURES: u64 = 1;
pub const VIRTGPU_PARAM_CAPSET_QUERY_FIX: u64 = 2;
pub const VIRTGPU_PARAM_RESOURCE_BLOB: u64 = 3;
pub const VIRTGPU_PARAM_HOST_VISIBLE: u64 = 4;
pub const VIRTGPU_PARAM_CROSS_DEVICE: u64 = 5;
pub const VIRTGPU_PARAM_CONTEXT_INIT: u64 = 6;
pub const VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS: u64 = 7;
pub const VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME: u64 = 8;

pub const VIRTGPU_CONTEXT_PARAM_CAPSET_ID: u64 = 0x0001;
pub const VIRTGPU_CONTEXT_PARAM_NUM_RINGS: u64 = 0x0002;
pub const VIRTGPU_CONTEXT_PARAM_POLL_RINGS_MASK: u64 = 0x0003;
pub const VIRTGPU_CONTEXT_PARAM_DEBUG_NAME: u64 = 0x0004;

pub const VIRTIO_GPU_CONTEXT_INIT_CAPSET_ID_MASK: u32 = 0x000000ff;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuGetParam {
    pub param: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuResourceCreate {
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
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuGetCaps {
    pub cap_set_id: u32,
    pub cap_set_ver: u32,
    pub addr: u64,
    pub size: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuMap {
    pub offset: u64,
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuExecbufferSyncobj {
    pub handle: u32,
    pub flags: u32,
    pub point: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuExecbuffer {
    pub flags: u32,
    pub size: u32,
    pub command: u64,
    pub bo_handles: u64,
    pub num_bo_handles: u32,
    pub fence_fd: i32,
    pub ring_idx: u32,
    pub syncobj_stride: u32,
    pub num_in_syncobjs: u32,
    pub num_out_syncobjs: u32,
    pub in_syncobjs: u64,
    pub out_syncobjs: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpu3dBox {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuTransferFromHost {
    pub bo_handle: u32,
    pub box_: VirtioGpu3dBox,
    pub level: u32,
    pub offset: u32,
    pub stride: u32,
    pub layer_stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuTransferToHost {
    pub bo_handle: u32,
    pub box_: VirtioGpu3dBox,
    pub level: u32,
    pub offset: u32,
    pub stride: u32,
    pub layer_stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuWait {
    pub handle: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuResourceCreateBlob {
    pub blob_mem: u32,
    pub blob_flags: u32,
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u64,
    pub pad: u32,
    pub cmd_size: u32,
    pub cmd: u64,
    pub blob_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuContextSetParam {
    pub param: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuContextInit {
    pub num_params: u32,
    pub pad: u32,
    pub ctx_set_params: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuResourceInfo {
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u32,
    pub blob_mem: u32,
}

pub mod gem;
mod output;

const XRES_MIN: u32 = 32;
const YRES_MIN: u32 = 32;
const XRES_DEF: u32 = 1024;
const YRES_DEF: u32 = 768;
const XRES_MAX: u32 = 8192;
const YRES_MAX: u32 = 8192;

#[derive(Debug)]
pub(crate) struct VirtioDrmDevice {
    device: Arc<DrmDevice>,

    has_virgl_3d: bool,
    has_edid: bool,
    has_indirect: bool,
    has_resource_assign_uuid: bool,
    has_resource_blob: bool,
}

impl VirtioDrmDevice {
    fn new(index: u32, gpu_device: Arc<dyn GpuDevice>) -> Result<Self, DrmError> {
        // acquire a typed Arc to the virtio GPU device
        // safe because the driver registration ensures this is the right type
        let vgpu = Arc::downcast::<VirtioGpuDevice>(gpu_device.clone()).unwrap();

        // initial mode_config
        let num_scanout: u32 = vgpu.num_scanouts();
        let mut mode_config = DrmModeConfig::new(
            XRES_MIN,
            XRES_MAX,
            YRES_MIN,
            YRES_MAX,
            16,
            Box::new(VirtioGpuModeConfigFuncs {}),
        );

        for scanout in 0..num_scanout {
            virtio_gpu_output_init(scanout, &mut mode_config, vgpu.clone())?;
        }

        mode_config.init_standard_properties();

        let driver = Arc::new(VirtioGpuDrmDrvier {});
        let driver_features = driver.driver_features();
        let device = Arc::new(DrmDevice::new(
            index,
            gpu_device,
            driver,
            driver_features,
            mode_config,
        ));

        let virtio_device = Self {
            device,

            has_virgl_3d: vgpu.has_virgl_3d(),
            has_edid: vgpu.has_edid(),
            has_indirect: vgpu.has_indirect(),
            has_resource_assign_uuid: vgpu.has_resource_assign_uuid(),
            has_resource_blob: vgpu.has_resource_blob(),
        };

        Ok(virtio_device)
    }
}

#[derive(Debug)]
pub(crate) struct VirtioGpuDrmDrvier;

impl DrmDriver for VirtioGpuDrmDrvier {
    fn name(&self) -> &str {
        DRIVER_NAME
    }

    fn desc(&self) -> &str {
        DRIVER_DESC
    }

    fn date(&self) -> &str {
        DRIVER_DATE
    }

    fn create_device(
        &self,
        index: u32,
        gpu_device: Arc<dyn GpuDevice>,
    ) -> Result<Arc<DrmDevice>, DrmError> {
        let virtio_device = VirtioDrmDevice::new(index, gpu_device)?;
        Ok(virtio_device.device)
    }

    fn driver_features(&self) -> DrmDriverFeatures {
        DrmDriverFeatures::GEM
            | DrmDriverFeatures::MODESET
            | DrmDriverFeatures::RENDER
            | DrmDriverFeatures::ATOMIC
            | DrmDriverFeatures::SYNCOBJ
            | DrmDriverFeatures::SYNCOBJ_TIMELINE
            | DrmDriverFeatures::CURSOR_HOTSPOT
    }

    fn driver_ops(&self) -> DrmDriverOps {
        DrmDriverOps {
            dumb_create: Some(DumbCreateProvider::Custom(
                virtio_gpu_mode_dumb_create_unreachable,
            )),
        }
    }
}

#[derive(Debug)]
struct VirtioGpuModeConfigFuncs {}

impl ModeConfigFuncs for VirtioGpuModeConfigFuncs {
    fn create_framebuffer(
        &self,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<aster_gpu::drm::gem::DrmGemObject>,
    ) -> Result<DrmFramebuffer, DrmError> {
        drm_gem_fb_create_with_dirty(width, height, pitch, bpp, gem_obj)
    }

    fn atomic_commit(&self, nonblock: bool) -> Result<(), DrmError> {
        drm_atomic_helper_commit(nonblock)
    }

    fn atomic_commit_tail(&self) -> Result<(), DrmError> {
        todo!()
    }
}
