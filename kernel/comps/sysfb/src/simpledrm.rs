use alloc::{boxed::Box, sync::Arc};

use aster_gpu::{
    GpuDevice,
    drm::{
        device::DrmDevice,
        driver::{DrmDriver, DrmDriverFeatures, DrmDriverOps, DumbCreateProvider},
        mode_config::{
            DrmModeModeInfo,
            connector::{ConnectorStatus, DrmConnector, funcs::ConnectorFuncs},
            crtc::{DrmCrtc, funcs::CrtcFuncs},
            encoder::{DrmEncoder, EncoderType, funcs::EncoderFuncs},
            plane::{DrmPlane, PlaneType, funcs::PlaneFuncs},
        },
    },
    drm_register_driver,
};
use ostd::boot::boot_info;

const SIMPLEDRM_NAME: &'static str = "simpledrm";
const SIMPLEDRM_DESC: &'static str = "DRM driver for simple-framebuffer platform devices";
const SIMPLEDRM_DATE: &'static str = "2025-01-02";

#[derive(Debug)]
pub struct SimpleDrmDevice {
    device: Arc<DrmDevice>,
}

impl SimpleDrmDevice {
    fn new(index: u32) -> Self {
        let driver = Arc::new(SimpleDrmDriver {});
        // TODO: initialize device-specific features
        // In Linux, a drm_device's features are not necessarily identical to
        // its driver's features. Here we start from the driver-wide features
        // and adjust per-device settings (e.g., enable/disable render node
        // or other capabilities for this specific device instance).
        let driver_features = driver.driver_features();
        let device = Arc::new(DrmDevice::new(index, driver, driver_features));

        Self { device }
    }

    fn init(&self) -> Result<(), ()> {
        let mut resources = self.device.resources().lock();
        resources.init_standard_properties();

        let primary_plane = DrmPlane::init(
            &mut resources,
            PlaneType::Primary,
            Box::new(SimplePlaneFuncs),
        )?;
        let crtc = DrmCrtc::init_with_planes(
            &mut resources,
            None,
            primary_plane,
            None,
            Box::new(SimpleCrtcFuncs),
        )?;
        let encoder = DrmEncoder::init_with_crtcs(
            &mut resources,
            EncoderType::VIRTUAL,
            &[crtc],
            Box::new(SimpleEncoderFuncs),
        )?;

        let fake_modeinfo = fake_modeinfo();
        let _connector = DrmConnector::init_with_encoder(
            &mut resources,
            ConnectorStatus::Connected,
            &[fake_modeinfo],
            &[encoder],
            Box::new(SimpleConnectorFuncs),
        )?;

        Ok(())
    }
}

drm_register_driver!(SimpleDrmDriver, SIMPLEDRM_NAME);

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

    fn create_device(&self, index: u32) -> Result<Arc<DrmDevice>, ()> {
        let sdev = SimpleDrmDevice::new(index);
        sdev.init()?;
        Ok(sdev.device.clone())
    }

    fn driver_features(&self) -> DrmDriverFeatures {
        DrmDriverFeatures::ATOMIC | DrmDriverFeatures::GEM | DrmDriverFeatures::MODESET
    }

    fn driver_ops(&self) -> DrmDriverOps {
        DrmDriverOps {
            dumb_create: Some(DumbCreateProvider::Memfd),
            ioctl: None,
            framebuffer_info: None,
        }
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

impl CrtcFuncs for SimpleCrtcFuncs {}

impl EncoderFuncs for SimpleEncoderFuncs {}

impl ConnectorFuncs for SimpleConnectorFuncs {}

#[derive(Debug)]
struct SimpleGpuDevice;

impl GpuDevice for SimpleGpuDevice {
    fn driver_name(&self) -> &str {
        SIMPLEDRM_NAME
    }
}

pub fn init() {
    if boot_info().framebuffer_arg.is_none() {
        return;
    }
    let device = Arc::new(SimpleGpuDevice {});
    let driver = Arc::new(SimpleDrmDriver {});
    aster_gpu::register_driver(SIMPLEDRM_NAME, driver)
        .expect("failed to register simple_drm DrmDriver");
    aster_gpu::register_device(device).expect("failed to register simple_drm GpuDevice");
}

// Create a fake display mode for testing and bring-up purposes.
//
// This mode is not obtained from real hardware (e.g. EDID or firmware).
// It provides a minimal, hard-coded timing description that allows the
// DRM pipeline to be exercised during early development, testing, or
// virtualized environments (such as simpledrm, QEMU, or headless setups).
//
// The values are chosen to represent a common 1280x800@60Hz mode and are
// sufficient for validating mode-setting, atomic state transitions, and
// userspace interaction. Real drivers must replace this with modes derived
// from hardware capabilities or display discovery mechanisms.
fn fake_modeinfo() -> DrmModeModeInfo {
    let mut name = [0u8; 32];
    let bytes = "1280x800".as_bytes();
    let len = bytes.len().min(32);
    name[..len].copy_from_slice(&bytes[..len]);

    DrmModeModeInfo {
        clock: 65000, // kHz (65 MHz)

        hdisplay: 1280,
        hsync_start: 1048,
        hsync_end: 1184,
        htotal: 1344,

        hskew: 0,

        vdisplay: 800,
        vsync_start: 771,
        vsync_end: 777,
        vtotal: 806,

        vscan: 0,

        vrefresh: 60,

        flags: 0x5,  // DRM_MODE_FLAG_PHSYNC | DRM_MODE_FLAG_PVSYNC
        type_: 0x40, // DRM_MODE_TYPE_DRIVER (0x40) or DRIVER | PREFERRED (0x60)

        name,
    }
}
