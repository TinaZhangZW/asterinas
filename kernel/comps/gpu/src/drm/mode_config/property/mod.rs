use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{any::Any, fmt::Debug};

use ostd::Pod;

pub const DRM_PROP_NAME_LEN: usize = 32;

fn str_to_u8_32(s: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let bytes = s.as_bytes();
    let len = core::cmp::min(bytes.len(), 32);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

bitflags::bitflags! {
    pub struct PropertyFlags: u32 {
        const PENDING    = 1 << 0;
        const RANGE      = 1 << 1;
        const IMMUTABLE  = 1 << 2;
        const ENUM       = 1 << 3;
        const BLOB       = 1 << 4;
        const BITMASK    = 1 << 5;

        const ATOMIC     = 0x8000_0000;
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum DrmModeObjectType {
    Any = 0,
    Crtc = 0xCCCC_CCCC,
    Connector = 0xC0C0_C0C0,
    Encoder = 0xE0E0_E0E0,
    Mode = 0xDEDE_DEDE,
    Property = 0xB0B0_B0B0,
    FB = 0xFBFB_FBFB,
    Blob = 0xBBBB_BBBB,
    Plane = 0xEEEE_EEEE,
}

#[derive(Debug)]
pub enum PropertyKind {
    Range { min: u64, max: u64 },
    SignedRange { min: i64, max: i64 },
    Enum(Vec<(u64, String)>),
    Bitmask(Vec<(u64, String)>),
    Blob,
    Object(DrmModeObjectType),
}

#[derive(Debug)]
pub struct DrmModeBlob {
    id: u32,
    data: Arc<[u8]>,
}

#[derive(Debug)]
pub struct DrmProperty {
    name: [u8; DRM_PROP_NAME_LEN],
    flags: PropertyFlags,
    kind: PropertyKind,
}

impl DrmProperty {
    pub fn name(&self) -> [u8; DRM_PROP_NAME_LEN] {
        self.name
    }

    pub fn flags(&self) -> u32 {
        self.flags.bits()
    }

    pub fn kind(&self) -> &PropertyKind {
        &self.kind
    }

    pub fn create(name: &str, flags: PropertyFlags) -> Self {
        Self::create_blob(name, flags)
    }

    pub fn create_blob(name: &str, flags: PropertyFlags) -> Self {
        Self {
            name: str_to_u8_32(name),
            flags: flags | PropertyFlags::BLOB,
            kind: PropertyKind::Blob,
        }
    }

    pub fn create_bool(name: &str, flags: PropertyFlags) -> Self {
        Self::create_range(name, flags, 0, 1)
    }

    pub fn create_range(name: &str, flags: PropertyFlags, min: u64, max: u64) -> Self {
        Self {
            name: str_to_u8_32(name),
            flags: flags | PropertyFlags::RANGE,
            kind: PropertyKind::Range { min, max },
        }
    }

    pub fn create_enum(name: &str, flags: PropertyFlags, enums: &[(u64, &str)]) -> Self {
        Self {
            name: str_to_u8_32(name),
            flags: flags | PropertyFlags::ENUM,
            kind: PropertyKind::Enum(enums.iter().map(|(v, s)| (*v, s.to_string())).collect()),
        }
    }

    pub fn create_signed_range(name: &str, flags: PropertyFlags, min: i64, max: i64) -> Self {
        Self {
            name: str_to_u8_32(name),
            flags,
            kind: PropertyKind::SignedRange { min, max },
        }
    }

    pub fn create_object(name: &str, flags: PropertyFlags, obj_type: DrmModeObjectType) -> Self {
        Self {
            name: str_to_u8_32(name),
            flags,
            kind: PropertyKind::Object(obj_type),
        }
    }

    pub fn has_name(&self, name: &str) -> bool {
        let bytes = name.as_bytes();
        let len = bytes.len().min(DRM_PROP_NAME_LEN);
        self.name[..len] == bytes[..len]
            && (len == DRM_PROP_NAME_LEN || self.name[len] == 0)
    }

    pub fn count_values(&self) -> u32 {
        match &self.kind {
            PropertyKind::Range { .. } => 2,
            PropertyKind::SignedRange { .. } => 2,
            PropertyKind::Enum(entries) => entries.len() as u32,
            PropertyKind::Bitmask(entries) => entries.len() as u32,
            PropertyKind::Blob => 0,
            PropertyKind::Object { .. } => 1,
        }
    }

    pub fn count_enum_blobs(&self) -> u32 {
        match &self.kind {
            PropertyKind::Enum(entries) => entries.len() as u32,
            PropertyKind::Bitmask(entries) => entries.len() as u32,
            // Linux GETPROPERTY reports no enum/blob entries for BLOB properties.
            PropertyKind::Blob => 0,
            _ => 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, Pod)]
pub struct PropertyEnum {
    value: u64,
    name: [u8; DRM_PROP_NAME_LEN],
}

impl PropertyEnum {
    pub fn new(value: u64, name: &str) -> Self {
        Self {
            value,
            name: str_to_u8_32(name),
        }
    }
}

pub trait PropertySpec: Debug + Any {
    fn name(&self) -> &'static str;
    fn build(&self) -> DrmProperty;
}
