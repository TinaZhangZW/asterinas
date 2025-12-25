// SPDX-License-Identifier: MPL-2.0

use alloc::{sync::Arc, vec::Vec};
use core::{any::Any, fmt::Debug};

use ostd::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    Errno(i32),
}

#[derive(Debug, Default)]
pub struct GpuResources {
    //pub planes: Vec<super::plane::Plane>,
    //pub crtcs: Vec<super::crtc::Crtc>,
    //pub connectors: Vec<super::connector::Connector>,
    //pub encoders: Vec<super::encoder::ModeEncoder>,
}

pub trait GpuDevice: Send + Sync + Any + Debug {
    /// Device name (for debugging / identification).
    fn name(&self) -> &str;

    /// Called by DRM during init_in_first_kthread().
    fn init(&self, _index: u32) -> Result<(), DeviceError> {
        Ok(())
    }

    /// Handle device-specific command / ioctl.
    fn handle_command(&self, _cmd: u32, _data: *mut u8) -> Result<(), DeviceError> {
        Err(DeviceError::Errno(-1)) // Not implemented
    }

    fn resources(&self) -> &Mutex<GpuResources>;
}

#[derive(Debug, Default)]
pub(crate) struct GpuDevices {
    devices: Vec<Arc<dyn GpuDevice>>,
}

impl GpuDevices {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Snapshot (clone Arcs) so caller can use it after unlocking the mutex.
    pub fn snapshot(&self) -> Vec<Arc<dyn GpuDevice>> {
        self.devices.clone()
    }

    pub fn register_device(&mut self, device: Arc<dyn GpuDevice>) -> Result<(), super::Error> {
        // Simple duplicate policy: by name.
        // (Better long-term: add a stable device id and dedup by id.)
        if self.devices.iter().any(|d| d.name() == device.name()) {
            return Err(super::Error::AlreadyRegistered);
        }
        self.devices.push(device);
        Ok(())
    }

    pub fn unregister_device(
        &mut self,
        device: &Arc<dyn GpuDevice>,
    ) -> Result<Arc<dyn GpuDevice>, super::Error> {
        if let Some(pos) = self.devices.iter().position(|d| Arc::ptr_eq(d, device)) {
            Ok(self.devices.remove(pos))
        } else {
            Err(super::Error::NotFound)
        }
    }
}
