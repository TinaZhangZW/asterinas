// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Arc;

use aster_pci::{
    capability::vendor::CapabilityVndrData,
    cfg_space::{Bar, MemoryBar},
    common_device::BarManager,
};
use log::warn;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
#[expect(clippy::enum_variant_names)]
pub enum VirtioPciCpabilityType {
    CommonCfg = 1,
    NotifyCfg = 2,
    IsrCfg = 3,
    DeviceCfg = 4,
    PciCfg = 5,
    SharedMemoryCfg = 8,
    VendorCfg = 9,
}

#[derive(Debug, Clone)]
pub struct VirtioPciCapabilityData {
    cfg_type: VirtioPciCpabilityType,
    id: u8,
    offset: u64,
    length: u64,
    option: Option<u32>,
    memory_bar: Option<Arc<MemoryBar>>,
}

impl VirtioPciCapabilityData {
    pub fn memory_bar(&self) -> &Option<Arc<MemoryBar>> {
        &self.memory_bar
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn id(&self) -> u8 {
        self.id
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn typ(&self) -> VirtioPciCpabilityType {
        self.cfg_type.clone()
    }

    pub fn option_value(&self) -> Option<u32> {
        self.option
    }

    pub(super) fn new(bar_manager: &BarManager, vendor_cap: CapabilityVndrData) -> Self {
        let cfg_type = vendor_cap.read8(3).unwrap();
        let cfg_type = match cfg_type {
            1 => VirtioPciCpabilityType::CommonCfg,
            2 => VirtioPciCpabilityType::NotifyCfg,
            3 => VirtioPciCpabilityType::IsrCfg,
            4 => VirtioPciCpabilityType::DeviceCfg,
            5 => VirtioPciCpabilityType::PciCfg,
            8 => VirtioPciCpabilityType::SharedMemoryCfg,
            9 => VirtioPciCpabilityType::VendorCfg,
            _ => panic!("Unsupported virtio capability type:{:?}", cfg_type),
        };
        let bar = vendor_cap.read8(4).unwrap();
        let id = vendor_cap.read8(5).unwrap();
        let capability_length = vendor_cap.read8(2).unwrap();
        let offset_lo = vendor_cap.read32(8).unwrap();
        let length_lo = vendor_cap.read32(12).unwrap();
        let (offset, length, option) = if cfg_type == VirtioPciCpabilityType::SharedMemoryCfg {
            if capability_length < 0x18 {
                warn!(
                    "virtio shared-memory capability is too short: len={}",
                    capability_length
                );
                (u64::from(offset_lo), u64::from(length_lo), None)
            } else {
                let offset_hi = vendor_cap.read32(16).unwrap();
                let length_hi = vendor_cap.read32(20).unwrap();
                (
                    u64::from(offset_lo) | (u64::from(offset_hi) << 32),
                    u64::from(length_lo) | (u64::from(length_hi) << 32),
                    None,
                )
            }
        } else {
            (
                u64::from(offset_lo),
                u64::from(length_lo),
                if capability_length > 0x10 {
                    Some(vendor_cap.read32(16).unwrap())
                } else {
                    None
                },
            )
        };

        let mut memory_bar = None;
        if let Some(bar) = bar_manager.bar(bar) {
            match bar {
                Bar::Memory(memory) => {
                    memory_bar = Some(memory);
                }
                Bar::Io(_) => {
                    warn!("`Bar::Io` is not supported")
                }
            }
        };
        Self {
            cfg_type,
            id,
            offset,
            length,
            option,
            memory_bar: memory_bar.cloned(),
        }
    }
}
