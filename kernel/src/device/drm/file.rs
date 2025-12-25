// SPDX-License-Identifier: MPL-2.0

extern crate alloc;

use alloc::{boxed::Box, format, string::String, sync::Arc};

use aster_gpu::{DeviceError, GpuDevice};
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

const DRM_MAJOR_ID: u16 = 226;

#[derive(Debug)]
pub struct DrmCardNode {
    index: u32,
}

impl DrmCardNode {
    pub fn new(index: u32) -> Self {
        Self { index }
    }
}

impl Device for DrmCardNode {
    fn type_(&self) -> DeviceType {
        DeviceType::Char
    }

    fn id(&self) -> DeviceId {
        DeviceId::new(MajorId::new(DRM_MAJOR_ID), MinorId::new(self.index))
    }

    fn devtmpfs_path(&self) -> Option<String> {
        Some(format!("dri/card{}", self.index))
    }

    fn open(&self) -> Result<Box<dyn FileIo>> {
        let device = aster_gpu::registered_devices()
            .get(self.index as usize)
            .cloned()
            .ok_or_else(|| Error::with_message(Errno::ENODEV, "drm: gpu device not found"))?;

        Ok(Box::new(DrmCardHandle { device }))
    }
}

struct DrmCardHandle {
    device: Arc<dyn GpuDevice>,
}

impl Pollable for DrmCardHandle {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }
}

impl DrmCardHandle {
    // Additional methods can be added here if needed
}

impl InodeIo for DrmCardHandle {
    fn read_at(
        &self,
        offset: usize,
        writer: &mut VmWriter,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        crate::return_errno_with_message!(Errno::EINVAL, "drm: read not supported");
    }

    fn write_at(
        &self,
        offset: usize,
        reader: &mut VmReader,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        crate::return_errno_with_message!(Errno::EINVAL, "drm: write not supported");
    }
}

impl FileIo for DrmCardHandle {
    fn check_seekable(&self) -> Result<()> {
        Ok(())
    }

    fn is_offset_aware(&self) -> bool {
        true
    }

    fn mappable(&self) -> Result<Mappable> {
        crate::return_errno_with_message!(Errno::EINVAL, "drm: write not supported");
    }

    fn ioctl(&self, _raw_ioctl: RawIoctl) -> Result<i32> {
        // TODO: Call GpuDevice.handle_command() if it needs device specific ioctl handling.
        crate::return_errno_with_message!(Errno::EINVAL, "drm: ioctl not supported");
    }
}
