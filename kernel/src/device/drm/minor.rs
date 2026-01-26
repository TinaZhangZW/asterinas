use alloc::{format, sync::Arc};

use aster_gpu::drm::{
    device::DrmDevice,
    driver::{DrmDriver, DrmDriverFeatures},
    gem::DrmGemObject,
    mode_config::DrmModeConfig,
};
use device_id::{DeviceId, MajorId, MinorId};

use crate::{
    device::drm::file::DrmFile,
    fs::{
        device::{Device, DeviceType},
        inode_handle::FileIo,
    },
    prelude::*,
};

const DRM_MAJOR_ID: u16 = 226;
const RENDER_MINOR_BASE: u32 = 128;

#[derive(Debug, Clone, Copy)]
pub(super) enum DrmMinorType {
    Primary = 0,
    Control = 1,
    Render = 2,
    Accel = 32,
}

/// Represents a DRM minor node exposed to userspace (e.g. primary, render,
/// or control node).
///
/// A `DrmMinor` corresponds to a single character device registered under
/// `/dev/dri/` (such as `/dev/dri/cardX` or `/dev/dri/renderDX`). It does not
/// own hardware state by itself; instead, it provides a userspace-facing
/// access point with a specific permission and usage model.
///
/// Multiple `DrmMinor` instances may reference the same underlying
/// `DrmDevice`, sharing the same driver instance and global device state.
/// The semantic differences between minors (e.g. authentication requirements,
/// ioctl visibility, access restrictions) are expressed via `type_` and
/// enforced at the file/ioctl level.
#[derive(Debug)]
pub(super) struct DrmMinor {
    /// The same index as DrmDevice<D>
    index: u32,
    /// The type of this minor node (primary, render, control, etc).
    ///
    /// This determines permission checks, supported ioctl sets,
    /// and access semantics enforced by the DRM core.
    type_: DrmMinorType,

    device: Arc<DrmDevice>,

    weak_self: Weak<Self>,
}

impl DrmMinor {
    pub fn new(device: Arc<DrmDevice>, type_: DrmMinorType) -> Arc<Self> {
        Arc::new_cyclic(move |weak_ref| Self {
            index: device.index(),
            type_,
            device,
            weak_self: weak_ref.clone(),
        })
    }

    pub fn driver(&self) -> Arc<dyn DrmDriver> {
        self.device.driver()
    }

    pub fn resources(&self) -> &Mutex<DrmModeConfig> {
        &self.device.resources()
    }

    pub fn check_feature(&self, features: DrmDriverFeatures) -> bool {
        self.device.check_feature(features)
    }

    pub fn create_offset(&self, gem_obj: Arc<DrmGemObject>) -> u64 {
        self.device.create_offset(gem_obj)
    }

    pub fn lookup_offset(&self, offset: &u64) -> Option<Arc<DrmGemObject>> {
        self.device.lookup_offset(offset)
    }

    pub fn remove_offset(&self, gem_obj: &Arc<DrmGemObject>) {
        self.device.remove_offset(gem_obj);
    }
}

impl Device for DrmMinor {
    fn type_(&self) -> DeviceType {
        DeviceType::Char
    }

    fn id(&self) -> DeviceId {
        let mut minor_id = self.index;
        match self.type_ {
            DrmMinorType::Render => {
                minor_id += RENDER_MINOR_BASE;
            }
            _ => {}
        }
        DeviceId::new(MajorId::new(DRM_MAJOR_ID), MinorId::new(minor_id))
    }

    fn devtmpfs_path(&self) -> Option<String> {
        match self.type_ {
            DrmMinorType::Primary => Some(format!("dri/card{}", self.index)),
            DrmMinorType::Render => Some(format!("dri/render{}", RENDER_MINOR_BASE + self.index)),
            DrmMinorType::Control => Some(format!("dri/controlD{}", self.index)),
            _ => None,
        }
    }

    fn open(&self) -> Result<Box<dyn FileIo>> {
        Ok(Box::new(DrmFile::new(self.weak_self.upgrade().unwrap())))
    }
}
