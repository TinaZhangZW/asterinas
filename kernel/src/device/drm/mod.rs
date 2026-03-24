mod file;
mod ioctl_defs;
mod memfd;
mod minor;
mod sysfs;

use aster_gpu::drm::{device::DrmDevice, driver::DrmDriverFeatures};

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
    for (index, gpu_device) in gpus.iter().enumerate() {
        if let Some(driver) = driver_table.get(gpu_device.driver_name()) {
            match driver.create_device(index as u32, gpu_device.clone()) {
                Ok(device) => {
                    any_success = true;
                    drm_dev_register(device)?;
                    // println!("[kernel] gpu device: {:?} probe correctly!", device.name());
                }
                Err(_error) => {
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
        char::register(drm_minor.clone())?;
        sysfs::register_minor(&drm_minor)?;
    } else {
        if device.check_feature(DrmDriverFeatures::RENDER) {
            let drm_minor = DrmMinor::new(device.clone(), DrmMinorType::Render);
            char::register(drm_minor.clone())?;
            sysfs::register_minor(&drm_minor)?;
        }

        let drm_minor = DrmMinor::new(device.clone(), DrmMinorType::Primary);
        char::register(drm_minor.clone())?;
        sysfs::register_minor(&drm_minor)?;
    }

    Ok(())
}
