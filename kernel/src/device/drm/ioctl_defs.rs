use aster_gpu::drm::ioctl::*;
use aster_virtio::device::gpu::drm::{VirtioGpuGetCaps, VirtioGpuGetParam};
use ostd::Pod;

use crate::util::ioctl::{InData, InOutData, NoData, ioc};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub(super) struct DrmAuth {
    pub magic: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub(super) struct DrmUnique {
    pub unique_len: i32,
    pub unique: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub(super) struct DrmSetVersion {
    pub drm_di_major: i32,
    pub drm_di_minor: i32,
    pub drm_dd_major: i32,
    pub drm_dd_minor: i32,
}

pub(super) type DrmIoctlVersion                 = ioc!(DRM_IOCTL_VERSION,                   b'd', 0x00, InOutData<DrmVersion>);
pub(super) type DrmIoctlGetUnique               = ioc!(DRM_IOCTL_GET_UNIQUE,                b'd', 0x01, InOutData<DrmUnique>);
pub(super) type DrmIoctlGetMagic                = ioc!(DRM_IOCTL_GET_MAGIC,                 b'd', 0x02, InOutData<DrmAuth>);
pub(super) type DrmIoctlSetVersion              = ioc!(DRM_IOCTL_SET_VERSION,               b'd', 0x07, InOutData<DrmSetVersion>);
pub(super) type DrmIoctlAuthMagic               = ioc!(DRM_IOCTL_AUTH_MAGIC,                b'd', 0x11, InData<DrmAuth>);
pub(super) type DrmIoctlGetCap                  = ioc!(DRM_IOCTL_GET_CAP,                   b'd', 0x0c, InOutData<DrmGetCap>);
pub(super) type DrmIoctlSetClientCap            = ioc!(DRM_IOCTL_SET_CLIENT_CAP,            b'd', 0x0d, InData<DrmSetClientCap>);
pub(super) type DrmIoctlSetMaster               = ioc!(DRM_IOCTL_SET_MASTER,                b'd', 0x1e, NoData);
pub(super) type DrmIoctlDropMaster              = ioc!(DRM_IOCTL_DROP_MASTER,               b'd', 0x1f, NoData);
pub(super) type DrmIoctlModeGetResources        = ioc!(DRM_IOCTL_MODE_GETRESOURCES,         b'd', 0xa0, InOutData<DrmModeGetResources>);
pub(super) type DrmIoctlModeGetCrtc             = ioc!(DRM_IOCTL_MODE_GETCRTC,              b'd', 0xa1, InOutData<DrmModeCrtc>);
pub(super) type DrmIoctlModeSetCrtc             = ioc!(DRM_IOCTL_MODE_SETCRTC,              b'd', 0xa2, InOutData<DrmModeCrtc>);
pub(super) type DrmIoctlModeCursor              = ioc!(DRM_IOCTL_MODE_CURSOR,               b'd', 0xa3, InOutData<DrmModeCursor>);
pub(super) type DrmIoctlSetGamma                = ioc!(DRM_IOCTL_SET_GAMMA,                 b'd', 0xa5, InOutData<DrmModeCrtcLut>);
pub(super) type DrmIoctlModeGetEncoder          = ioc!(DRM_IOCTL_MODE_GETENCODER,           b'd', 0xa6, InOutData<DrmModeGetEncoder>);
pub(super) type DrmIoctlModeGetConnector        = ioc!(DRM_IOCTL_MODE_GETCONNECTOR,         b'd', 0xa7, InOutData<DrmModeGetConnector>);
pub(super) type DrmIoctlModeGetProperty         = ioc!(DRM_IOCTL_MODE_GETPROPERTY,          b'd', 0xaa, InOutData<DrmModeGetProperty>);
pub(super) type DrmIoctlModeSetProperty         = ioc!(DRM_IOCTL_MODE_SETPROPERTY,          b'd', 0xab, InOutData<DrmModeConnectorSetProperty>);
pub(super) type DrmIoctlModeGetPropBlob         = ioc!(DRM_IOCTL_MODE_GETPROPBLOB,          b'd', 0xac, InOutData<DrmModeGetBlob>);
pub(super) type DrmIoctlModeAddFB               = ioc!(DRM_IOCTL_MODE_ADDFB,                b'd', 0xae, InOutData<DrmModeFBCmd>);
pub(super) type DrmIoctlModeRmFB                = ioc!(DRM_IOCTL_MODE_RMFB,                 b'd', 0xaf, InData<DrmModeFBCmd>);
pub(super) type DrmIoctlModeDirtyFb             = ioc!(DRM_IOCTL_MODE_DIRTYFB,              b'd', 0xb1, InOutData<DrmModeFbDirtyCmd>);
pub(super) type DrmIoctlModeCreateDumb          = ioc!(DRM_IOCTL_MODE_CREATE_DUMB,          b'd', 0xb2, InOutData<DrmModeCreateDumb>);
pub(super) type DrmIoctlModeMapDumb             = ioc!(DRM_IOCTL_MODE_MAP_DUMB,             b'd', 0xb3, InOutData<DrmModeMapDumb>);
// The `DrmModeDestroyDumb` struct is a single `u32`.  On 64‑bit targets
// it used to be padded to eight bytes which made `ioc!` generate the
// wrong ioctl value (0xc00864b4) and prevented our handler from matching.
// We solved that by annotating the struct with `#[repr(C, packed)]` in the
// `aster-gpu` crate so that `size_of::<DrmModeDestroyDumb>() == 4`.  The
// normal `ioc!` helper now computes the correct command value, so we can
// use a plain type alias instead of hard‑coding a constant.

pub(super) type DrmIoctlModeDestroyDumb         =
    ioc!(DRM_IOCTL_MODE_DESTROY_DUMB,         b'd', 0xb4, InOutData<DrmModeDestroyDumb>);
pub(super) type DrmIoctlModeGetPlaneResources   = ioc!(DRM_IOCTL_MODE_GETPLANERESOURCES,    b'd', 0xb5, InOutData<DrmModeGetPlaneRes>);
pub(super) type DrmIoctlModeGetPlane            = ioc!(DRM_IOCTL_MODE_GETPLANE,             b'd', 0xb6, InOutData<DrmModeGetPlane>);
pub(super) type DrmIoctlModeObjectGetProps      = ioc!(DRM_IOCTL_MODE_OBJ_GETPROPERTIES,    b'd', 0xb9, InOutData<DrmModeObjectGetProps>);
pub(super) type DrmIoctlModeCursor2             = ioc!(DRM_IOCTL_MODE_CURSOR2,              b'd', 0xbb, InOutData<DrmModeCursor>);
pub(super) type DrmIoctlVirtioGpuGetParam       = ioc!(DRM_IOCTL_VIRTGPU_GETPARAM,          b'd', 0x43, InOutData<VirtioGpuGetParam>);
pub(super) type DrmIoctlVirtioGpuGetCaps        = ioc!(DRM_IOCTL_VIRTGPU_GET_CAPS,          b'd', 0x49, InOutData<VirtioGpuGetCaps>);
