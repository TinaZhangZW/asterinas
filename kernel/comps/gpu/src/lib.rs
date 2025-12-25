// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

mod gpu_dev;

use alloc::{sync::Arc, vec::Vec};

use component::{ComponentInitError, init_component};
pub use gpu_dev::{DeviceError, GpuDevice, GpuResources};
use ostd::sync::Mutex;
use spin::Once;

/// Error type for GPU device registry operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    AlreadyRegistered,
    NotFound,
}

/// Registers a GPU device.
pub fn register_device(device: Arc<dyn GpuDevice>) -> Result<(), Error> {
    let component = COMPONENT
        .get()
        .expect("aster-gpu component not initialized");
    component.gpu_devices.lock().register_device(device)
}

/// Unregisters a GPU device.
pub fn unregister_device(device: &Arc<dyn GpuDevice>) -> Result<Arc<dyn GpuDevice>, Error> {
    let component = COMPONENT
        .get()
        .expect("aster-gpu component not initialized");
    component.gpu_devices.lock().unregister_device(device)
}

/// Returns a snapshot of all registered GPU devices.
pub fn registered_devices() -> Vec<Arc<dyn GpuDevice>> {
    let component = COMPONENT
        .get()
        .expect("aster-gpu component not initialized");
    component.gpu_devices.lock().snapshot()
}

static COMPONENT: Once<Component> = Once::new();

#[init_component]
fn component_init() -> Result<(), ComponentInitError> {
    let component = Component::init()?;
    COMPONENT.call_once(|| component);
    Ok(())
}

#[derive(Debug)]
struct Component {
    gpu_devices: Mutex<gpu_dev::GpuDevices>,
}

impl Component {
    fn init() -> Result<Self, ComponentInitError> {
        Ok(Self {
            gpu_devices: Mutex::new(gpu_dev::GpuDevices::new()),
        })
    }
}
