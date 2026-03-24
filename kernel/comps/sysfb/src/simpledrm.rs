use alloc::{boxed::Box, sync::Arc};

use aster_framebuffer::FRAMEBUFFER;
use aster_gpu::{
    GpuDevice,
    drm::{
        DrmError,
        device::DrmDevice,
        driver::{DrmDriver, DrmDriverFeatures, DrmDriverOps, DumbCreateProvider},
        gem::DrmGemObject,
        ioctl::DrmModeCrtc,
        mode_config::{
            DrmModeConfig,
            connector::{
                ConnectorStatus, DrmConnector,
                funcs::{ConnectorFuncs, drm_helper_probe_single_connector_modes},
            },
            crtc::{DrmCrtc, funcs::CrtcFuncs, helper::drm_atomic_helper_page_flip},
            encoder::{DrmEncoder, EncoderType, funcs::EncoderFuncs},
            framebuffer::{DrmFramebuffer, helper::drm_gem_fb_create_with_dirty},
            funcs::{ModeConfigFuncs, drm_atomic_helper_commit},
            plane::{DrmPlane, PlaneType, funcs::PlaneFuncs},
        },
        vblank::DrmPendingVblankEvent,
    },
};
use ostd::mm::io_util::HasVmReaderWriter;

use crate::helper::{drm_sysfb_connector_helper_get_modes, drm_sysfb_gem_create};

const SIMPLEDRM_NAME: &'static str = "simpledrm";
const SIMPLEDRM_DESC: &'static str = "DRM driver for simple-framebuffer platform devices";
const SIMPLEDRM_DATE: &'static str = "2025-01-02";

#[derive(Debug)]
pub struct SimpleDrmDevice {
    device: Arc<DrmDevice>,
}

impl SimpleDrmDevice {
    fn new(index: u32, gpu_device: Arc<dyn GpuDevice>) -> Result<Self, DrmError> {
        // TODO: get the hardware format to set this properties
        let min_width = 1;
        let max_width = 8192;
        let min_height = 1;
        let max_height = 8192;
        let preferred_depth = 16;

        let mut mode_config = DrmModeConfig::new(
            min_width,
            max_width,
            min_height,
            max_height,
            preferred_depth,
            Box::new(SimpleModeConfigFuncs {}),
        );

        // Drm Objects initial
        let primary_plane = DrmPlane::init(
            &mut mode_config,
            PlaneType::Primary,
            Box::new(SimplePlaneFuncs),
        )?;
        let crtc = DrmCrtc::init_with_planes(
            &mut mode_config,
            None,
            primary_plane,
            None,
            Box::new(SimpleCrtcFuncs),
        )?;
        let encoder = DrmEncoder::init_with_crtcs(
            &mut mode_config,
            EncoderType::VIRTUAL,
            &[crtc],
            Box::new(SimpleEncoderFuncs),
        )?;

        let _connector = DrmConnector::init_with_encoder(
            &mut mode_config,
            &[encoder],
            Box::new(SimpleConnectorFuncs),
        )?;

        mode_config.init_standard_properties();

        let driver = Arc::new(SimpleDrmDriver {});
        // TODO: initialize device-specific features
        // In Linux, a drm_device's features are not necessarily identical to
        // its driver's features. Here we start from the driver-wide features
        // and adjust per-device settings (e.g., enable/disable render node
        // or other capabilities for this specific device instance).
        let driver_features = driver.driver_features();
        let device = Arc::new(DrmDevice::new(
            index,
            gpu_device,
            driver,
            driver_features,
            mode_config,
        ));

        Ok(Self { device })
    }
}

#[derive(Debug)]
struct SimpleDrmDriver {}

impl DrmDriver for SimpleDrmDriver {
    fn name(&self) -> &str {
        SIMPLEDRM_NAME
    }

    fn desc(&self) -> &str {
        SIMPLEDRM_DESC
    }

    fn date(&self) -> &str {
        SIMPLEDRM_DATE
    }

    fn create_device(
        &self,
        index: u32,
        gpu_device: Arc<dyn GpuDevice>,
    ) -> Result<Arc<DrmDevice>, DrmError> {
        let sdev = SimpleDrmDevice::new(index, gpu_device)?;
        Ok(sdev.device.clone())
    }

    fn driver_features(&self) -> DrmDriverFeatures {
        DrmDriverFeatures::GEM | DrmDriverFeatures::MODESET
    }

    fn driver_ops(&self) -> DrmDriverOps {
        DrmDriverOps {
            dumb_create: Some(DumbCreateProvider::MemfdBackend(drm_sysfb_gem_create)),
        }
    }
}

#[derive(Debug)]
struct SimpleModeConfigFuncs;

impl ModeConfigFuncs for SimpleModeConfigFuncs {
    fn create_framebuffer(
        &self,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
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

#[derive(Debug)]
struct SimplePlaneFuncs;

#[derive(Debug)]
struct SimpleCrtcFuncs;

#[derive(Debug)]
struct SimpleEncoderFuncs;

#[derive(Debug)]
struct SimpleConnectorFuncs;

impl PlaneFuncs for SimplePlaneFuncs {}

impl CrtcFuncs for SimpleCrtcFuncs {
    fn page_flip(
        &self,
        device: Arc<DrmDevice>,
        crtc: Arc<DrmCrtc>,
        fb: Arc<DrmFramebuffer>,
        event: Option<DrmPendingVblankEvent>,
        flags: u32,
        target: Option<u32>,
    ) -> Result<(), DrmError> {
        drm_atomic_helper_page_flip(device, crtc, fb, event, flags, target)
    }

    fn set_config(
        &self,
        _crtc: Arc<DrmCrtc>,
        fb: Arc<DrmFramebuffer>,
        _crtc_req: &DrmModeCrtc,
    ) -> Result<(), DrmError> {
        // TODO: Now just legacy achievement, copy the drm_framebuffer
        // to firmware_framebuffer
        if let Some(framebuffer) = FRAMEBUFFER.get() {
            let iomem = framebuffer.io_mem();
            let mut writer = iomem.writer().to_fallible();
            fb.read(0, &mut writer)?;
        } else {
            return Err(DrmError::NotFound);
        }

        Ok(())
    }

    fn enable_vblank(&self, _crtc: Arc<DrmCrtc>) -> Result<(), DrmError> {
        todo!()
    }

    fn disable_vblank(&self, _crtc: Arc<DrmCrtc>) -> Result<(), DrmError> {
        todo!()
    }
}

impl EncoderFuncs for SimpleEncoderFuncs {}

impl ConnectorFuncs for SimpleConnectorFuncs {
    fn fill_modes(
        &self,
        max_x: u32,
        max_y: u32,
        connector: Arc<DrmConnector>,
    ) -> Result<(), DrmError> {
        drm_helper_probe_single_connector_modes(max_x, max_y, connector)
    }

    fn detect(&self, _force: bool, connector: Arc<DrmConnector>) -> Result<(), DrmError> {
        // TODO: dirty method
        connector.update_status(ConnectorStatus::Connected)
    }

    fn get_modes(&self, connector: Arc<DrmConnector>) -> Result<(), DrmError> {
        drm_sysfb_connector_helper_get_modes(connector)
    }
}

#[derive(Debug)]
struct SimpleGpuDevice;

impl GpuDevice for SimpleGpuDevice {
    fn driver_name(&self) -> &str {
        SIMPLEDRM_NAME
    }
}

pub fn register_device() {
    let device = Arc::new(SimpleGpuDevice {});
    aster_gpu::register_device(device).expect("failed to register simple_drm GpuDevice");
}

pub fn register_driver() {
    let driver = Arc::new(SimpleDrmDriver {});
    aster_gpu::register_driver(SIMPLEDRM_NAME, driver)
        .expect("failed to register simple_drm DrmDriver");
}
