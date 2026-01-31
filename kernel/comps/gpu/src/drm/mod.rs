//! DRM core interfaces and registries for GPU drivers.
//!
//! This module provides the DRM driver registry and foundational types used
//! by driver implementations. The registry is used by the GPU component to
//! match discovered devices to compatible drivers.

use alloc::{
    string::{String, ToString},
    sync::Arc,
};

use hashbrown::HashMap;

use crate::drm::driver::DrmDriver;

pub mod device;
pub mod driver;
pub mod gem;
pub mod mode_config;

/// Registry of DRM drivers keyed by name.
#[derive(Debug, Default)]
pub(crate) struct DrmDrivers {
    drivers: HashMap<String, Arc<dyn DrmDriver>>,
}

impl DrmDrivers {
    pub fn new() -> Self {
        Self {
            drivers: HashMap::new(),
        }
    }

    /// Returns a snapshot of the current registry.
    ///
    /// The returned map contains `Arc` clones to allow use after releasing
    /// the registry lock.
    pub fn snapshot(&self) -> HashMap<String, Arc<dyn DrmDriver>> {
        self.drivers.clone()
    }

    /// Registers a DRM driver under `name`.
    ///
    /// If a driver already exists under the same name, it is replaced.
    pub fn register_driver(
        &mut self,
        name: &str,
        driver: Arc<dyn DrmDriver>,
    ) -> Result<(), super::Error> {
        self.drivers.insert(name.to_string(), driver);
        Ok(())
    }

    /// Unregisters the DRM driver named `name`.
    pub fn unregister_driver(&mut self, name: &str) -> Result<Arc<dyn DrmDriver>, super::Error> {
        if let Some(driver) = self.drivers.remove(name) {
            Ok(driver)
        } else {
            Err(super::Error::NotFound)
        }
    }
}
