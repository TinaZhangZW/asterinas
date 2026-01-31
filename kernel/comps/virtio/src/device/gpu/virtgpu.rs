// SPDX-License-Identifier: MPL-2.0

//! Virtio-GPU DRM uAPI definitions.
//!
//! This module mirrors the Linux `virtgpu_drm.h` ABI for ioctl payloads and
//! command numbers so that userspace built against the standard headers can
//! interoperate with the Asterinas DRM subsystem.

use core::mem::size_of;

use ostd::Pod;

/// DRM ioctl base magic for GPU devices.
const DRM_IOCTL_BASE: u8 = b'd';
/// Driver-specific ioctl base (DRM_COMMAND_BASE in Linux DRM).
const DRM_COMMAND_BASE: u8 = 0x40;

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, type_: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (type_ << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT)
}

const fn iowr(nr: u8, size: u32) -> u32 {
    ioc(
        IOC_READ | IOC_WRITE,
        DRM_IOCTL_BASE as u32,
        nr as u32,
        size,
    )
}

/// ioctl command numbers (raw encoded values).
pub const VIRTGPU_MAP: u32 = iowr(DRM_COMMAND_BASE + 0x00, size_of::<DrmVirtgpuMap>() as u32);
pub const VIRTGPU_EXECBUFFER: u32 =
    iowr(DRM_COMMAND_BASE + 0x01, size_of::<DrmVirtgpuExecbuffer>() as u32);
pub const VIRTGPU_GETPARAM: u32 =
    iowr(DRM_COMMAND_BASE + 0x02, size_of::<DrmVirtgpuGetparam>() as u32);
pub const VIRTGPU_RESOURCE_CREATE: u32 =
    iowr(DRM_COMMAND_BASE + 0x03, size_of::<DrmVirtgpuResourceCreate>() as u32);
pub const VIRTGPU_RESOURCE_INFO: u32 =
    iowr(DRM_COMMAND_BASE + 0x04, size_of::<DrmVirtgpuResourceInfo>() as u32);
pub const VIRTGPU_TRANSFER_FROM_HOST: u32 =
    iowr(DRM_COMMAND_BASE + 0x05, size_of::<DrmVirtgpuTransfer>() as u32);
pub const VIRTGPU_TRANSFER_TO_HOST: u32 =
    iowr(DRM_COMMAND_BASE + 0x06, size_of::<DrmVirtgpuTransfer>() as u32);
pub const VIRTGPU_WAIT: u32 = iowr(DRM_COMMAND_BASE + 0x07, size_of::<DrmVirtgpuWait>() as u32);
pub const VIRTGPU_GET_CAPS: u32 =
    iowr(DRM_COMMAND_BASE + 0x08, size_of::<DrmVirtgpuGetCaps>() as u32);
pub const VIRTGPU_RESOURCE_CREATE_BLOB: u32 =
    iowr(DRM_COMMAND_BASE + 0x09, size_of::<DrmVirtgpuResourceCreateBlob>() as u32);
pub const VIRTGPU_CONTEXT_INIT: u32 =
    iowr(DRM_COMMAND_BASE + 0x0a, size_of::<DrmVirtgpuContextInit>() as u32);
pub const VIRTGPU_RESOURCE_UNREF: u32 =
    iowr(DRM_COMMAND_BASE + 0x0b, size_of::<DrmVirtgpuResourceUnref>() as u32);

/// Values for `DrmVirtgpuGetparam::param`.
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum VirtgpuParam {
    /// Returns non-zero if 3D/virgl is supported.
    Param3dFeatures = 1,
    /// Maximum supported capset ID.
    CapsetMaxId = 2,
    /// Maximum supported capset version.
    CapsetMaxVersion = 3,
    /// Maximum supported capset size.
    CapsetMaxSize = 4,
    /// Whether context init is supported.
    ContextInit = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuMap {
    pub offset: u64,
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuExecbuffer {
    pub flags: u32,
    pub size: u32,
    pub command: u64,
    pub fence_id: u64,
    pub ring_idx: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuGetparam {
    pub param: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuResourceCreate {
    pub target: u32,
    pub format: u32,
    pub bind: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuResourceInfo {
    pub bo_handle: u32,
    pub res_handle: u32,
    pub size: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuTransfer {
    pub bo_handle: u32,
    pub res_handle: u32,
    pub level: u32,
    pub stride: u32,
    pub layer_stride: u32,
    pub box_: DrmVirtgpuRect,
    pub offset: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuWait {
    pub handle: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuGetCaps {
    pub caps: u64,
    pub size: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuResourceCreateBlob {
    pub blob_mem: u32,
    pub blob_flags: u32,
    pub blob_id: u32,
    pub pad: u32,
    pub size: u64,
    pub bo_handle: u32,
    pub res_handle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuContextInit {
    pub ctx_id: u32,
    pub num_capsets: u32,
    pub capsets: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Default)]
pub struct DrmVirtgpuResourceUnref {
    pub res_handle: u32,
    pub pad: u32,
}
