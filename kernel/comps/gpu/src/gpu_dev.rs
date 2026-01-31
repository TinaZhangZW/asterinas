// SPDX-License-Identifier: MPL-2.0

use alloc::{sync::Arc, vec::Vec};
use core::{any::Any, fmt::Debug};

/// A low-level abstraction for a GPU-capable device discovered by the system.
///
/// `GpuDevice` is implemented by bus- or platform-specific objects (for example
/// PCI, Virtio, or firmware-provided devices) to advertise GPU capability. The
/// trait is designed **only for driver matching and probing**, not for device
/// lifetime management or DRM node representation.
///
/// Typical flow:
/// 1. A concrete device (e.g., `VirtioGpuDevice`) is discovered by its bus.
/// 2. The device implements `GpuDevice` to declare GPU capability.
/// 3. The DRM core selects a compatible `DrmDriver` using device properties.
/// 4. One or more `DrmDevice` instances are created and bound to the driver.
///
/// Notes:
/// - `GpuDevice` does not represent a DRM device node.
/// - `GpuDevice` does not handle char device registration or file operations.
/// - A single `GpuDevice` may map to multiple DRM nodes (primary/render/control).
pub trait GpuDevice: Send + Sync + Any + Debug {
    /// Returns the driver name used for matching this device to a DRM driver.
    ///
    /// The returned string is expected to correspond to a name registered via
    /// the DRM driver registry. Implementations should return a stable value.
    fn driver_name(&self) -> &str;
    // more settings e.g. device_id, capability, resources
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

    /// Returns a snapshot of registered devices.
    ///
    /// The returned vector contains `Arc` clones to allow use after releasing
    /// the registry lock.
    pub fn snapshot(&self) -> Vec<Arc<dyn GpuDevice>> {
        self.devices.clone()
    }

    pub fn register_device(&mut self, device: Arc<dyn GpuDevice>) -> Result<(), super::Error> {
        // TODO: Simple duplicate policy: add a stable device id and dedup by id.
        // TODO: remove simpledrm device if any true gpu device registered
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
