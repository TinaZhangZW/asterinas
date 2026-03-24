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
pub const DRM_VIRTGPU_GET_CAPS: u8 = 0x09;

pub const VIRTGPU_PARAM_3D_FEATURES: u64 = 1;
pub const VIRTGPU_PARAM_CAPSET_QUERY_FIX: u64 = 2;
pub const VIRTGPU_PARAM_RESOURCE_BLOB: u64 = 3;
pub const VIRTGPU_PARAM_HOST_VISIBLE: u64 = 4;
pub const VIRTGPU_PARAM_CROSS_DEVICE: u64 = 5;
pub const VIRTGPU_PARAM_CONTEXT_INIT: u64 = 6;
pub const VIRTGPU_PARAM_SUPPORTED_CAPSET_IDS: u64 = 7;
pub const VIRTGPU_PARAM_EXPLICIT_DEBUG_NAME: u64 = 8;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct VirtioGpuGetParam {
    pub param: u64,
    pub value: u64,
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
