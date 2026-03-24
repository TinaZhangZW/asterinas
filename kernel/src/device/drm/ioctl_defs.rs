use aster_gpu::drm::ioctl::*;
use aster_virtio::device::gpu::drm::{
    VirtioGpuContextInit, VirtioGpuExecbuffer, VirtioGpuGetCaps, VirtioGpuGetParam, VirtioGpuMap,
    VirtioGpuResourceCreateBlob,
    VirtioGpuResourceCreate,
    VirtioGpuTransferFromHost,
    VirtioGpuTransferToHost,
    VirtioGpuResourceInfo,
    VirtioGpuWait,
};

use crate::util::ioctl::{InData, InOutData, NoData, ioc};

pub(super) type DrmIoctlVersion                 = ioc!(DRM_IOCTL_VERSION,                   b'd', 0x00, InOutData<DrmVersion>);
pub(super) type DrmIoctlGetCap                  = ioc!(DRM_IOCTL_GET_CAP,                   b'd', 0x0c, InOutData<DrmGetCap>);
pub(super) type DrmIoctlSetClientCap            = ioc!(DRM_IOCTL_SET_CLIENT_CAP,            b'd', 0x0d, InData<DrmSetClientCap>);
pub(super) type DrmIoctlGemClose               = ioc!(DRM_IOCTL_GEM_CLOSE,                 b'd', 0x09, InData<DrmGemClose>);
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
pub(super) type DrmIoctlVirtioGpuExecbuffer     = ioc!(DRM_IOCTL_VIRTGPU_EXECBUFFER,        b'd', 0x42, InOutData<VirtioGpuExecbuffer>);
pub(super) type DrmIoctlVirtioGpuResourceCreate = ioc!(DRM_IOCTL_VIRTGPU_RESOURCE_CREATE,   b'd', 0x44, InOutData<VirtioGpuResourceCreate>);
pub(super) type DrmIoctlVirtioGpuTransferFromHost = ioc!(DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST, b'd', 0x46, InOutData<VirtioGpuTransferFromHost>);
pub(super) type DrmIoctlVirtioGpuTransferToHost = ioc!(DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST, b'd', 0x47, InOutData<VirtioGpuTransferToHost>);
pub(super) type DrmIoctlVirtioGpuWait           = ioc!(DRM_IOCTL_VIRTGPU_WAIT, b'd', 0x48, InOutData<VirtioGpuWait>);
pub(super) type DrmIoctlVirtioGpuGetCaps        = ioc!(DRM_IOCTL_VIRTGPU_GET_CAPS,          b'd', 0x49, InOutData<VirtioGpuGetCaps>);
pub(super) type DrmIoctlVirtioGpuResourceCreateBlob = ioc!(DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB, b'd', 0x4a, InOutData<VirtioGpuResourceCreateBlob>);
pub(super) type DrmIoctlVirtioGpuContextInit    = ioc!(DRM_IOCTL_VIRTGPU_CONTEXT_INIT,      b'd', 0x4b, InOutData<VirtioGpuContextInit>);
pub(super) type DrmIoctlVirtioGpuMap            = ioc!(DRM_IOCTL_VIRTGPU_MAP,               b'd', 0x41, InOutData<VirtioGpuMap>);
pub(super) type DrmIoctlVirtioGpuResourceInfo   = ioc!(DRM_IOCTL_VIRTGPU_RESOURCE_INFO,    b'd', 0x45, InOutData<VirtioGpuResourceInfo>);
pub(super) type DrmIoctlSyncobjCreate           = ioc!(DRM_IOCTL_SYNCOBJ_CREATE,            b'd', 0xbf, InOutData<DrmSyncobjCreate>);
pub(super) type DrmIoctlSyncobjDestroy          = ioc!(DRM_IOCTL_SYNCOBJ_DESTROY,           b'd', 0xc0, InData<DrmSyncobjDestroy>);
pub(super) type DrmIoctlSyncobjHandleToFd       = ioc!(DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD,      b'd', 0xc1, InOutData<DrmSyncobjHandle>);
pub(super) type DrmIoctlSyncobjFdToHandle       = ioc!(DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE,      b'd', 0xc2, InOutData<DrmSyncobjHandle>);
pub(super) type DrmIoctlSyncobjWait             = ioc!(DRM_IOCTL_SYNCOBJ_WAIT,              b'd', 0xc3, InOutData<DrmSyncobjWait>);
pub(super) type DrmIoctlSyncobjReset            = ioc!(DRM_IOCTL_SYNCOBJ_RESET,             b'd', 0xc4, InData<DrmSyncobjArray>);
pub(super) type DrmIoctlSyncobjSignal           = ioc!(DRM_IOCTL_SYNCOBJ_SIGNAL,            b'd', 0xc5, InData<DrmSyncobjArray>);
pub(super) type DrmIoctlSyncobjTimelineWait     = ioc!(DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT,     b'd', 0xca, InOutData<DrmSyncobjTimelineWait>);
pub(super) type DrmIoctlSyncobjQuery            = ioc!(DRM_IOCTL_SYNCOBJ_QUERY,             b'd', 0xcb, InData<DrmSyncobjTimelineArray>);
pub(super) type DrmIoctlSyncobjTransfer         = ioc!(DRM_IOCTL_SYNCOBJ_TRANSFER,          b'd', 0xcc, InData<DrmSyncobjTransfer>);
pub(super) type DrmIoctlSyncobjTimelineSignal   = ioc!(DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL,   b'd', 0xcd, InData<DrmSyncobjTimelineArray>);
pub(super) type DrmIoctlSyncobjEventfd          = ioc!(DRM_IOCTL_SYNCOBJ_EVENTFD,           b'd', 0xcf, InData<DrmSyncobjEventfd>);
