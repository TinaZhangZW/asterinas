// SPDX-License-Identifier: MPL-2.0

pub mod config;
pub mod device;
pub mod drm;
pub mod sysfs;
pub mod virtgpu;

pub const DEVICE_NAME: &str = "Virtio-GPU";

pub const VIRTIO_GPU_DRIVER_NAME: &str = "virtio-gpu";
pub const VIRTIO_GPU_DRIVER_DESC: &str = "Virtio GPU DRM driver";
pub const VIRTIO_GPU_DRIVER_DATE: &str = "2026-01-31";
