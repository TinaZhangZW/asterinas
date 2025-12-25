// SPDX-License-Identifier: MPL-2.0

mod file;
mod simple_drm;

use crate::error::Errno;

pub(super) fn init_in_first_kthread() -> Result<(), super::Error> {
    simple_drm::init();

    let devices = aster_gpu::registered_devices();
    if devices.is_empty() {
        return Err(super::Error::with_message(
            Errno::ENODEV,
            "no GPU devices registered",
        ));
    }

    if devices
        .into_iter()
        .enumerate()
        .any(|(index, dev)| dev.init(index as u32).is_ok())
    {
        Ok(())
    } else {
        Err(super::Error::with_message(
            Errno::EIO,
            "all GPU device initializations failed",
        ))
    }
}
