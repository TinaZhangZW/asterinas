use int_to_c_enum::TryFromInt;
use ostd::Pod;

use crate::drm::mode_config::{DrmModeModeInfo, property::DRM_PROP_NAME_LEN};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: u64,
    pub name: u64,
    pub date_len: u64,
    pub date: u64,
    pub desc_len: u64,
    pub desc: u64,
}

impl DrmVersion {
    pub fn is_first_call(&self) -> bool {
        self.name == 0 && self.date == 0 && self.desc == 0
    }
}

#[repr(u64)]
#[derive(Debug, TryFromInt)]
pub enum DrmCapabilities {
    DumbBuffer = 0x1,
    VblankHighCrtc = 0x2,
    DumbPreferredDepth = 0x3,
    DumbPreferShadow = 0x4,
    Prime = 0x5,
    TimestampMonotonic = 0x6,
    AsyncPageFlip = 0x7,
    CursorWidth = 0x8,
    CursorHeight = 0x9,
    Addfb2Modifiers = 0x10,
    PageFlipTarget = 0x11,
    CrtcInVblankEvent = 0x12,
    SyncObj = 0x13,
    SyncObjTimeline = 0x14,
    AtomicAsyncPageFlip = 0x15,
}

bitflags::bitflags! {
    pub struct DrmPrimeValue: u64 {
        const IMPORT = 0x1;
        const EXPORT = 0x2;
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmGetCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(u64)]
#[derive(Debug, TryFromInt)]
pub enum ClientCaps {
    Stereo3D = 0x1,
    UniversalPlane = 0x2,
    Atomic = 0x3,
    AspectRatio = 0x4,
    WritebackConnectors = 0x5,
    CursorPlaneHostport = 0x6,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSetClientCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetResources {
    pub fb_id_ptr: u64,
    pub crtc_id_ptr: u64,
    pub connector_id_ptr: u64,
    pub encoder_id_ptr: u64,

    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,

    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

impl DrmModeGetResources {
    pub fn is_first_call(&self) -> bool {
        return self.fb_id_ptr == 0
            && self.crtc_id_ptr == 0
            && self.connector_id_ptr == 0
            && self.encoder_id_ptr == 0;
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeCrtc {
    pub set_connectors_ptr: u64,
    pub count_connectors: u32,

    pub crtc_id: u32,
    ///framebuffer
    pub fb_id: u32,

    ///tion on the framebuffer
    pub x: u32,
    ///tion on the framebuffer
    pub y: u32,

    pub gamma_size: u32,
    pub mode_valid: u32,
    pub mode: DrmModeModeInfo,
}

#[repr(u32)]
#[derive(Debug, TryFromInt)]
pub enum DrmModeCursorFlags {
    Bo = 0x1,
    Move = 0x2,
    Flags = 0x3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeCursor {
    pub flags: u32,
    pub crtc_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /* driver specific handle */
    pub handle: u32,
    pub hot_x: i32,
    pub hot_y: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeCrtcLut {
    pub crtc_id: u32,
    pub gamma_size: u32,

    /* pointers to arrays */
    pub red: u64,
    pub green: u64,
    pub blue: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetEncoder {
    pub encoder_id: u32,
    pub encoder_type: u32,

    pub crtc_id: u32,
    /**< Id of crtc */
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetConnector {
    /// Pointer to array of encoder IDs.
    pub encoders_ptr: u64,
    /// Pointer to array of drm_mode_modeinfo.
    pub modes_ptr: u64,
    /// Pointer to array of property IDs.
    pub props_ptr: u64,
    /// Pointer to array of property values.
    pub prop_values_ptr: u64,

    pub count_modes: u32,
    pub count_props: u32,
    pub count_encoders: u32,

    /// ID of the current encoder.
    pub encoder_id: u32,
    /// ID of the connector.
    pub connector_id: u32,

    /// Type of the connector.
    /// See DrmModeConnStatus
    pub connector_type: u32,

    /// This is a per-type connector number.
    pub connector_type_id: u32,

    /// Status of the connector.
    /// See DrmModeConnStatus
    pub connection: u32,
    /// Width of the connected sink in millimeters.
    pub mm_width: u32,
    /// Height of the connected sink in millimeters.
    pub mm_height: u32,

    /// Subpixel order of the connected sink.
    /// See enum SubpixelOrder.
    pub subpixel: u32,

    /// Padding, must be zero.
    pub pad: u32,
}

impl DrmModeGetConnector {
    pub fn is_first_call(&self) -> bool {
        return self.encoders_ptr == 0
            && self.modes_ptr == 0
            && self.props_ptr == 0
            && self.prop_values_ptr == 0;
    }
}

/// User-space can perform a GETPROPERTY ioctl to retrieve information about a
/// property. The same property may be attached to multiple objects, see
/// "Modeset Base Object Abstraction".
/// User-space is expected to retrieve values and enums by performing this ioctl
/// at least twice: the first time to retrieve the number of elements, the
/// second time to retrieve the elements themselves.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetProperty {
    /// Pointer to a ``__u64`` array.
    pub values_ptr: u64,
    /// Pointer to a struct drm_mode_property_enum array.
    pub enum_blob_ptr: u64,

    /// Object ID of the property which should be retrieved.
    /// Set by the caller.
    pub prop_id: u32,
    /// ``DRM_MODE_PROP_*`` bitfield. See &drm_property.flags for
    /// a definition of the flags.
    pub flags: u32,
    /// Symbolic property name. User-space should use this field to
    /// recognize properties.
    pub name: [u8; DRM_PROP_NAME_LEN],

    /// Number of elements in values_ptr.
    pub count_values: u32,
    /// Number of elements in enum_blob_ptr.
    pub count_enum_blobs: u32,
}

impl DrmModeGetProperty {
    pub fn is_first_call(&self) -> bool {
        return self.values_ptr == 0 && self.enum_blob_ptr == 0;
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeConnectorSetProperty {
    value: u64,
    prop_id: u32,
    connector_id: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Pod)]
pub struct DrmModeGetBlob {
    pub blob_id: u32,
    pub length: u32,
    pub data: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeFBCmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    /* driver specific handle */
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeFbDirtyCmd {
    pub fb_id: u32,
    pub flags: u32,
    pub color: u32,
    pub num_clips: u32,
    pub clips_ptr: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeCreateDumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    /* handle, pitch, size will be returned */
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeMapDumb {
    /** Handle for the object being mapped. */
    pub handle: u32,
    pub pad: u32,
    /**
     * Fake offset to use for subsequent mmap call
     *
     * This is a fixed-size type for 32/64 compatibility.
     */
    pub offset: u64,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeDestroyDumb {
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmGemClose {
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetPlaneRes {
    pub plane_id_ptr: u64,
    pub count_planes: u32,
}

impl DrmModeGetPlaneRes {
    pub fn is_first_call(&self) -> bool {
        return self.plane_id_ptr == 0;
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeGetPlane {
    /// Object ID of the plane whose information should be
    /// retrieved. Set by caller.
    pub plane_id: u32,

    /// Object ID of the current CRTC.
    pub crtc_id: u32,
    /// Object ID of the current fb.
    pub fb_id: u32,

    /// Bitmask of CRTC's compatible with the plane. CRTC's
    /// are created and they receive an index, which corresponds to their
    /// position in the bitmask. Bit N corresponds to
    /// :ref:`CRTC index<crtc_index>` N.
    pub possible_crtcs: u32,
    /// Never used.
    pub gamma_size: u32,

    /// Number of formats.
    pub count_format_types: u32,
    /// Pointer to ``__u32`` array of formats that are
    /// supported by the plane. These formats do not require modifiers.
    pub format_type_ptr: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmModeObjectGetProps {
    /// props_id array
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_props: u32,
    pub obj_id: u32,
    pub obj_type: u32,
}

impl DrmModeObjectGetProps {
    pub fn is_first_call(&self) -> bool {
        return self.props_ptr == 0 && self.prop_values_ptr == 0;
    }
}

pub const DRM_SYNCOBJ_CREATE_SIGNALED: u32 = 0x1;

pub const DRM_SYNCOBJ_FD_TO_HANDLE_FLAGS_IMPORT_SYNC_FILE: u32 = 0x1;
pub const DRM_SYNCOBJ_HANDLE_TO_FD_FLAGS_EXPORT_SYNC_FILE: u32 = 0x1;

pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL: u32 = 0x1;
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT: u32 = 0x2;
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_AVAILABLE: u32 = 0x4;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjCreate {
    pub handle: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjDestroy {
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjHandle {
    pub handle: u32,
    pub flags: u32,
    pub fd: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjWait {
    pub handles: u64,
    pub timeout_nsec: u64,
    pub count_handles: u32,
    pub flags: u32,
    pub first_signaled: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjTimelineWait {
    pub handles: u64,
    pub points: u64,
    pub timeout_nsec: u64,
    pub count_handles: u32,
    pub flags: u32,
    pub first_signaled: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjArray {
    pub handles: u64,
    pub count_handles: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjTimelineArray {
    pub handles: u64,
    pub points: u64,
    pub count_handles: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjTransfer {
    pub src_handle: u32,
    pub dst_handle: u32,
    pub src_point: u64,
    pub dst_point: u64,
    pub flags: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub struct DrmSyncobjEventfd {
    pub handle: u32,
    pub flags: u32,
    pub point: u64,
    pub fd: i32,
    pub pad: u32,
}
