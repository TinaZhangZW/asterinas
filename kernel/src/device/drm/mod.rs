mod minor;
mod file;
mod ioctl_defs;
mod memfd;

use aster_framebuffer::{FRAMEBUFFER, PixelFormat, register_external_from_paddr};
use aster_gpu::{GpuPixelFormat, drm::{device::DrmDevice, driver::DrmDriverFeatures}};
use log::warn;

use crate::{
    device::{
        drm::minor::{DrmMinor, DrmMinorType},
        registry::char,
    },
    prelude::*,
};

pub(super) fn init_in_first_kthread() -> Result<()> {
    let gpus = aster_gpu::registered_devices();
    let driver_table = aster_gpu::registered_drivers();

    if gpus.is_empty() {
        return_errno_with_message!(Errno::ENODEV, "no GPU devices registered");
    }

    let mut any_success = false;

    // TODO: Do not rely on device.name() for driver matching.
    //
    // Matching GpuDevice and DrmDriver, if matched, create DrmDevice.
    // Introduce a capability- or ID-based matching interface between GpuDevice and
    // DrmDriver to enable precise, extensible, and bus-agnostic driver selection.
    for (index, device) in gpus.iter().enumerate() {
        if let Some(driver) = driver_table.get(device.driver_name()) {
            match driver.create_device(index as u32) {
                Ok(device) => {
                    any_success = true;
                    drm_dev_register(device.clone())?;
                    try_register_fbdev(device.as_ref());
                }
                Err(_error) => {
                    warn!(
                        "[kernel] gpu device: driver {} failed to create device for index {}",
                        driver.name(),
                        index
                    );
                    // TODO: handle the error
                }
            }
        }
    }

    if any_success {
        Ok(())
    } else {
        return_errno_with_message!(Errno::ENODEV, "all GPU devices register failed");
    }
}

fn drm_dev_register(device: Arc<DrmDevice>) -> Result<()> {
    if device.check_feature(DrmDriverFeatures::COMPUTE_ACCEL) {
        let drm_minor = DrmMinor::new(device.clone(), DrmMinorType::Accel);
        char::register(drm_minor)?;
    } else {
        if device.check_feature(DrmDriverFeatures::RENDER) {
            let drm_minor = DrmMinor::new(device.clone(), DrmMinorType::Render);
            char::register(drm_minor)?;
        }

        let drm_minor = DrmMinor::new(device.clone(), DrmMinorType::Primary);
        char::register(drm_minor)?;
    }

    Ok(())
}

fn try_register_fbdev(device: &DrmDevice) {
    if FRAMEBUFFER.get().is_some() {
        return;
    }

    let Some(provider) = device.driver().driver_ops().framebuffer_info else {
        return;
    };

    let Some(info) = provider(device) else {
        warn!("fbdev: no framebuffer info from DRM driver");
        return;
    };

    let pixel_format = match info.pixel_format {
        GpuPixelFormat::Grayscale8 => PixelFormat::Grayscale8,
        GpuPixelFormat::Rgb565 => PixelFormat::Rgb565,
        GpuPixelFormat::Rgb888 => PixelFormat::Rgb888,
        GpuPixelFormat::BgrReserved => PixelFormat::BgrReserved,
    };

    if let Err(err) = register_external_from_paddr(
        info.paddr,
        info.size,
        info.width,
        info.height,
        info.line_size,
        pixel_format,
    ) {
        warn!("fbdev: failed to register framebuffer: {:?}", err);
    }
}
