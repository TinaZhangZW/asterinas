// SPDX-License-Identifier: MPL-2.0

extern crate alloc;

use alloc::{boxed::Box, format, string::String, sync::Arc};

use aster_gpu::{DeviceError, GpuDevice, GpuResources};
use device_id::{DeviceId, MajorId, MinorId};
use ostd::{
    Pod,
    mm::{HasPaddr, HasSize, VmIo, io_util::HasVmReaderWriter},
};

use crate::{
    current_userspace,
    device::registry::char,
    events::IoEvents,
    fs::{
        device::{Device, DeviceType},
        file_handle::Mappable,
        inode_handle::FileIo,
        utils::{InodeIo, StatusFlags},
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    util::ioctl::{RawIoctl, dispatch_ioctl},
};

#[derive(Debug)]
pub struct SimpleDrmDevice {
    name: &'static str,
    resources: Mutex<GpuResources>,
}

impl SimpleDrmDevice {
    pub fn new() -> Self {
        Self {
            name: "simple_drm",
            resources: Mutex::new(GpuResources::default()),
        }
    }
}

impl GpuDevice for SimpleDrmDevice {
    fn name(&self) -> &str {
        self.name
    }

    fn resources(&self) -> &Mutex<GpuResources> {
        &self.resources
    }

    fn init(&self, index: u32) -> core::result::Result<(), DeviceError> {
        // TODO: Initialize resources (planes, crtcs, connectors, encoders).
        // E.g.:
        //   let mut res = self.resources.lock();
        //   res.planes.push(Plane::new(...));
        //   res.crtcs.push(Crtc::new(...));

        char::register(Arc::new(super::file::DrmCardNode::new(index)))
            .map_err(|_| DeviceError::Errno(-(Errno::EIO as i32)))?;
        Ok(())
    }
}

pub(super) fn init() {
    let device = Arc::new(SimpleDrmDevice::new());
    aster_gpu::register_device(device).expect("failed to register simple_drm GpuDevice");
}
