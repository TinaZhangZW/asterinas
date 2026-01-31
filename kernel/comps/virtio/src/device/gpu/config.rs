// SPDX-License-Identifier: MPL-2.0

use core::mem::offset_of;

use aster_util::safe_ptr::SafePtr;
use ostd::Pod;

use crate::transport::{ConfigManager, VirtioTransport};

bitflags::bitflags! {
    /// Feature bits for a virtio-gpu device.
    pub struct VirtioGpuFeatures: u64 {
        const VIRGL = 1 << 0;
        const EDID = 1 << 1;
        const RESOURCE_UUID = 1 << 2;
        const RESOURCE_BLOB = 1 << 3;
        const CONTEXT_INIT = 1 << 4;
    }
}

#[derive(Debug, Pod, Clone, Copy)]
#[repr(C)]
pub struct VirtioGpuConfig {
    /// Bitmask of pending device events.
    pub events_read: u32,
    /// Write-back mask to clear device events.
    pub events_clear: u32,
    /// Number of display scanouts supported by the device.
    pub num_scanouts: u32,
    pub _reserved: u32,
}

impl VirtioGpuConfig {
    pub(super) fn new_manager(transport: &dyn VirtioTransport) -> ConfigManager<Self> {
        let safe_ptr = transport
            .device_config_mem()
            .map(|mem| SafePtr::new(mem, 0));
        let bar_space = transport.device_config_bar();
        ConfigManager::new(safe_ptr, bar_space)
    }
}

impl ConfigManager<VirtioGpuConfig> {
    pub(super) fn read_config(&self) -> VirtioGpuConfig {
        let mut cfg = VirtioGpuConfig::new_uninit();
        cfg.events_read = self
            .read_once::<u32>(offset_of!(VirtioGpuConfig, events_read))
            .unwrap();
        cfg.events_clear = self
            .read_once::<u32>(offset_of!(VirtioGpuConfig, events_clear))
            .unwrap();
        cfg.num_scanouts = self
            .read_once::<u32>(offset_of!(VirtioGpuConfig, num_scanouts))
            .unwrap();
        cfg._reserved = self
            .read_once::<u32>(offset_of!(VirtioGpuConfig, _reserved))
            .unwrap();
        cfg
    }

    /// Reads the device event bits.
    #[allow(dead_code)]
    pub(super) fn read_events(&self) -> u32 {
        self.read_once::<u32>(offset_of!(VirtioGpuConfig, events_read))
            .unwrap()
    }

    /// Clears the specified device event bits.
    #[allow(dead_code)]
    pub(super) fn clear_events(&self, mask: u32) {
        self.write_once(offset_of!(VirtioGpuConfig, events_clear), mask)
            .unwrap();
    }
}
