use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use aster_framebuffer::FRAMEBUFFER;
use aster_gpu::drm::{
    driver::{DrmDriverFeatures, DumbCreateProvider},
    gem::DrmGemObject,
    mode_config::{
        DrmModeModeInfo,
        property::{PropertyEnum, PropertyKind},
    },
};
use hashbrown::HashMap;
use ostd::mm::{VmIo, io_util::HasVmReaderWriter};

use crate::{
    current_userspace,
    device::drm::{
        minor::DrmMinor,
        ioctl_defs::*,
        memfd::{DrmMemfdFile, dumb_create_impl},
    },
    events::IoEvents,
    fs::{
        file_handle::Mappable,
        inode_handle::FileIo,
        utils::{InodeIo, StatusFlags},
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    util::ioctl::{RawIoctl, dispatch_ioctl},
};

/// Represents an open DRM file descriptor exposed to userspace.
///
/// `DrmFile` is created on each successful `open()` of a DRM device node
/// (e.g. `/dev/dri/cardX`, `/dev/dri/renderDX`). It serves as the **per-open
/// execution context** for all userspace interactions with the DRM subsystem.
///
/// Responsibilities:
/// - Dispatching ioctl requests issued from userspace.
/// - Enforcing access restrictions and semantics defined by the associated
///   DRM minor (primary, render, control, etc.).
///
/// `DrmFile` does not own device-wide state. Instead, it holds a reference to
/// the `DrmMinor` through which it was opened, and all operations are ultimately
/// routed to the underlying `DrmDevice` shared by all minors of the same GPU.
///
/// Each `DrmFile` instance is independent and represents a single userspace
/// file descriptor, while the underlying DRM device and driver state are
/// shared across all open files.
#[derive(Debug)]
pub(super) struct DrmFile {
    device: Arc<DrmMinor>,

    /// True when the client has asked us to expose stereo 3D mode flags.
    stereo_allowed: AtomicBool,
    /// True if client understands CRTC primary planes and cursor planes
    /// in the plane list. Automatically set when atomic is set.
    universal_planes: AtomicBool,
    /// True if client understands atomic properties.
    atomic: AtomicBool,
    /// True, if client can handle picture aspect ratios, and has requested
    /// to pass this information along with the mode.
    aspect_ratio_allowed: AtomicBool,
    /// True if client understands writeback connectors
    writeback_connectors: AtomicBool,
    /// This client is capable of handling the cursor plane with the
    /// restrictions imposed on it by the virtualized drivers.
    supports_virtualized_cursor_plane: AtomicBool,

    /// GEM objects are referenced by 32‑bit handles that
    /// are *per file descriptor*. Each open DRM file maintains its own
    /// namespace of GEM handles. This atomic counter is used to allocate
    /// unique handles for newly created GEM objects visible to userspace
    /// through this file.
    next_handle: AtomicU32,
    gem_table: Mutex<HashMap<u32, Arc<DrmGemObject>>>,
}

impl Pollable for DrmFile {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }
}

impl DrmFile {
    pub fn new(device: Arc<DrmMinor>) -> Self {
        Self {
            device,

            stereo_allowed: AtomicBool::new(false),
            universal_planes: AtomicBool::new(false),
            atomic: AtomicBool::new(false),
            aspect_ratio_allowed: AtomicBool::new(false),
            writeback_connectors: AtomicBool::new(false),
            supports_virtualized_cursor_plane: AtomicBool::new(false),

            next_handle: AtomicU32::new(1),
            gem_table: Mutex::new(HashMap::new()),
        }
    }

    fn next_handle(&self) -> u32 {
        self.next_handle.fetch_add(1, Ordering::SeqCst)
    }

    fn insert_gem(&self, handle: u32, gem_object: Arc<DrmGemObject>) {
        self.gem_table.lock().insert(handle, gem_object);
    }

    fn lookup_gem(&self, handle: &u32) -> Option<Arc<DrmGemObject>> {
        self.gem_table.lock().get(handle).cloned()
    }

    fn remove_gem(&self, handle: &u32) -> Option<Arc<DrmGemObject>> {
        self.gem_table.lock().remove(handle)
    }
}

impl InodeIo for DrmFile {
    fn read_at(
        &self,
        _offset: usize,
        _writer: &mut VmWriter,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        return_errno_with_message!(Errno::EINVAL, "drm: read not supported");
    }

    fn write_at(
        &self,
        _offset: usize,
        _reader: &mut VmReader,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        return_errno_with_message!(Errno::EINVAL, "drm: write not supported");
    }
}

impl FileIo for DrmFile {
    fn check_seekable(&self) -> Result<()> {
        Ok(())
    }

    fn is_offset_aware(&self) -> bool {
        true
    }

    fn mappable_with_offset(&self, offset: usize) -> Result<Mappable> {
        if let Some(gem_obj) = self.device.lookup_offset(&(offset as u64)) {
            if let Some(drm_memfd) = gem_obj.downcast_ref::<DrmMemfdFile>() {
                return drm_memfd.mappable();
            } else {
                // TODO: hardware memory mmap
            }
        }

        return_errno!(Errno::EINVAL);
    }

    fn ioctl(&self, raw_ioctl: RawIoctl) -> Result<i32> {
        // TODO: Call GpuDevice.handle_command() if it needs device specific ioctl handling.
        // TODO: drm_file permit flags check (master, root, render ...)
        dispatch_ioctl!(match raw_ioctl {
            cmd @ DrmIoctlVersion => {
                let mut user_data: DrmVersion = cmd.read()?;

                let driver = self.device.driver();

                let name = driver.name();
                let name_len = name.len() as u64;
                let desc = driver.desc();
                let desc_len = desc.len() as u64;
                let date = driver.date();
                let date_len = date.len() as u64;

                if user_data.is_first_call() {
                    user_data.name_len = name_len;
                    user_data.desc_len = desc_len;
                    user_data.date_len = date_len;

                    cmd.write(&user_data)?;
                } else {
                    // TODO: better write cstring method
                    // the name,desc,date now is u64, maybe should use cstring?
                    if user_data.name_len >= name_len {
                        current_userspace!()
                            .write_bytes(user_data.name as usize, name.as_bytes())?;
                    } else {
                        return_errno!(Errno::EINVAL);
                    }

                    if user_data.desc_len >= desc_len {
                        current_userspace!()
                            .write_bytes(user_data.desc as usize, desc.as_bytes())?;
                    } else {
                        return_errno!(Errno::EINVAL);
                    }

                    if user_data.date_len >= date_len {
                        current_userspace!()
                            .write_bytes(user_data.date as usize, date.as_bytes())?;
                    } else {
                        return_errno!(Errno::EINVAL);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlGetCap => {
                let mut user_data: DrmGetCap = cmd.read()?;

                let cap = DrmCapabilities::try_from(user_data.capability)?;

                let value = match cap {
                    DrmCapabilities::TimestampMonotonic => 1,
                    DrmCapabilities::Prime => {
                        (DrmPrimeValue::IMPORT | DrmPrimeValue::EXPORT).bits()
                    }
                    DrmCapabilities::SyncObj => {
                        self.device.check_feature(DrmDriverFeatures::SYNCOBJ) as u64
                    }
                    DrmCapabilities::SyncObjTimeline => self
                        .device
                        .check_feature(DrmDriverFeatures::SYNCOBJ_TIMELINE)
                        as u64,
                    _ => {
                        if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }

                        let mode_config = &self.device.resources().lock();

                        match cap {
                            DrmCapabilities::DumbBuffer => {
                                self.device.driver().driver_ops().dumb_create.is_some() as u64
                            }
                            DrmCapabilities::VblankHighCrtc => 1,
                            DrmCapabilities::DumbPreferredDepth => {
                                mode_config.preferred_depth as u64
                            }
                            DrmCapabilities::DumbPreferShadow => mode_config.prefer_shadow as u64,
                            DrmCapabilities::AsyncPageFlip => mode_config.async_page_flip as u64,
                            DrmCapabilities::PageFlipTarget => {
                                // TODO: check if each crtc has func: page_flip_target
                                0
                            }
                            DrmCapabilities::CursorWidth => match mode_config.cursor_width {
                                0 => 64,
                                w => w as u64,
                            },
                            DrmCapabilities::CursorHeight => match mode_config.cursor_height {
                                0 => 64,
                                h => h as u64,
                            },
                            DrmCapabilities::Addfb2Modifiers => {
                                !mode_config.fb_modifiers_not_supported as u64
                            }
                            DrmCapabilities::CrtcInVblankEvent => 1,
                            DrmCapabilities::AtomicAsyncPageFlip => {
                                (self.device.check_feature(DrmDriverFeatures::ATOMIC)
                                    && mode_config.async_page_flip)
                                    as u64
                            }
                            _ => 0,
                        }
                    }
                };

                user_data.value = value;

                cmd.write(&user_data)?;
                Ok(0)
            }
            cmd @ DrmIoctlSetClientCap => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmSetClientCap = cmd.read()?;

                match ClientCaps::try_from(user_data.capability)? {
                    ClientCaps::Stereo3D => match user_data.value {
                        0 | 1 => {
                            self.stereo_allowed
                                .store(user_data.value == 1, Ordering::Relaxed);
                        }
                        _ => return_errno!(Errno::EINVAL),
                    },
                    ClientCaps::UniversalPlane => {
                        match user_data.value {
                            0 | 1 => {
                                self.universal_planes
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::Atomic => {
                        if !self.device.check_feature(DrmDriverFeatures::ATOMIC) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }
                        // TODO: The modesetting DDX has a totally broken idea of atomic.
                        // if (current->comm[0] == 'X' && req->value == 1) {
                        // 	pr_info("broken atomic modeset userspace detected, disabling atomic\n");
                        //  return -EOPNOTSUPP;
                        // }

                        match user_data.value {
                            0 | 1 | 2 => {
                                let v = user_data.value;

                                self.atomic.store(v >= 1, Ordering::Relaxed);
                                self.universal_planes.store(v >= 1, Ordering::Relaxed);
                                self.aspect_ratio_allowed.store(v == 2, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        }
                    }
                    ClientCaps::AspectRatio => {
                        match user_data.value {
                            0 | 1 => {
                                self.aspect_ratio_allowed
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::WritebackConnectors => {
                        if !self.atomic.load(Ordering::Relaxed) {
                            return_errno!(Errno::EINVAL);
                        }

                        match user_data.value {
                            0 | 1 => {
                                self.writeback_connectors
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                    ClientCaps::CursorPlaneHostport => {
                        if !self.device.check_feature(DrmDriverFeatures::CURSOR_HOTSPOT) {
                            return_errno!(Errno::EOPNOTSUPP);
                        }

                        if !self.atomic.load(Ordering::Relaxed) {
                            return_errno!(Errno::EINVAL);
                        }

                        match user_data.value {
                            0 | 1 => {
                                self.supports_virtualized_cursor_plane
                                    .store(user_data.value == 1, Ordering::Relaxed);
                            }
                            _ => return_errno!(Errno::EINVAL),
                        };
                    }
                }

                Ok(0)
            }
            _cmd @ DrmIoctlSetMaster => {
                // TODO:
                Ok(0)
            }
            _cmd @ DrmIoctlDropMaster => {
                // TODO:
                Ok(0)
            }
            cmd @ DrmIoctlModeGetResources => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetResources = cmd.read()?;

                let res = self.device.resources().lock();

                let count_crtcs = res.count_crtcs();
                let count_encoders = res.count_encoders();
                let count_connectors = res.count_connectors();
                let count_fbs = res.count_framebuffers();

                if user_data.is_first_call() {
                    user_data.count_crtcs = count_crtcs;
                    user_data.count_encoders = count_encoders;
                    user_data.count_connectors = count_connectors;
                    user_data.count_fbs = count_fbs;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_connectors >= count_connectors {
                        for (i, id) in res.connectors_id().enumerate() {
                            let offset = user_data.connector_id_ptr as usize
                                + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_crtcs >= count_crtcs {
                        for (i, id) in res.crtcs_id().enumerate() {
                            let offset =
                                user_data.crtc_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_encoders >= count_encoders {
                        for (i, id) in res.encoders_id().enumerate() {
                            let offset =
                                user_data.encoder_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_fbs >= count_fbs {
                        for (i, id) in res.framebuffer_id().enumerate() {
                            let offset =
                                user_data.encoder_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetCrtc => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeCrtc = cmd.read()?;
                let crtc_id = user_data.crtc_id;
                let crtc = match self.device.resources().lock().get_crtc(&crtc_id) {
                    Some(c) => c,
                    None => {
                        return_errno!(Errno::ENOENT)
                    }
                };

                // TODO: Full mode validation and proper atomic handling:
                //
                // Current implementation only returns basic CRTC fields (gamma_size, fb_id, x/y).
                // It does not validate the mode, handle atomic commits, or propagate errors
                // for unsupported configurations. These behaviors are part of the standard
                // Linux DRM design and must be implemented for proper userspace interaction.
                user_data.gamma_size = crtc.gamma_size();
                user_data.fb_id = crtc.fb_id();
                (user_data.x, user_data.y) = crtc.xy();

                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeSetCrtc => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmModeCrtc = cmd.read()?;
                let fb_id = user_data.fb_id;

                // TODO: Now just legacy achievement, copy the drm_framebuffer
                // to firmware_framebuffer
                if let Some(framebuffer) = FRAMEBUFFER.get() {
                    let iomem = framebuffer.io_mem();
                    let mut writer = iomem.writer().to_fallible();

                    let mode_config = self.device.resources().lock();
                    if let Some(drm_framebuffer) = mode_config.lookup_framebuffer(&fb_id) {
                        // TODO: handle the error
                        let _ = drm_framebuffer.read(0, &mut writer);
                    } else {
                        return_errno!(Errno::ENOENT);
                    }
                } else {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeCursor => {
                let _user_data: DrmModeCursor = cmd.read()?;

                // TODO:
                // not support hardware cursor return ENXIO
                return_errno!(Errno::ENXIO);
            }
            cmd @ DrmIoctlModeCursor2 => {
                let _user_data: DrmModeCursor = cmd.read()?;

                // TODO:
                // not support hardware cursor return ENXIO
                return_errno!(Errno::ENXIO);
            }
            cmd @ DrmIoctlSetGamma => {
                let _user_data: DrmModeCrtcLut = cmd.read()?;

                // TODO:

                Ok(0)
            }
            cmd @ DrmIoctlModeGetEncoder => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetEncoder = cmd.read()?;
                let encoder_id = user_data.encoder_id;

                let encoder = match self.device.resources().lock().get_encoder(&encoder_id) {
                    Some(encoder) => encoder,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                // TODO: implement proper encoder state resolution including lease support.
                //
                // A lease allows a different DRM client (lessee) to take exclusive
                // control of certain objects. When querying the encoder’s current CRTC,
                // the core checks whether the file descriptor holds a lease on that CRTC.
                // If so, it returns the leased crtc_id;
                // otherwise it may return 0 (no binding).

                user_data.encoder_type = encoder.type_() as u32;
                user_data.encoder_id = encoder.id();
                user_data.possible_crtcs = encoder.possible_crtcs();
                user_data.possible_clones = encoder.possible_clones();

                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeGetConnector => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetConnector = cmd.read()?;
                let conn_id = user_data.connector_id;

                let conn = match self.device.resources().lock().get_connector(&conn_id) {
                    Some(conn) => conn,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_modes = conn.count_modes();
                let count_props = conn.count_props();
                let count_encoders = conn.count_encoders();

                if user_data.is_first_call() {
                    user_data.count_modes = count_modes;
                    user_data.count_props = count_props;
                    user_data.count_encoders = count_encoders;

                    user_data.connector_type = conn.type_() as u32;
                    user_data.connector_type_id = conn.type_id_();
                    user_data.connection = conn.status() as u32;

                    user_data.mm_width = conn.mm_width();
                    user_data.mm_height = conn.mm_height();
                    user_data.subpixel = conn.subpixel_order();
                    user_data.pad = 0;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_modes >= count_modes {
                        for (i, mode) in conn.modes().enumerate() {
                            let offset = user_data.modes_ptr as usize
                                + i * core::mem::size_of::<DrmModeModeInfo>();
                            current_userspace!().write_val(offset, mode)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_encoders >= count_encoders as u32 {
                        for (i, id) in conn.possible_encoders_id().enumerate() {
                            let offset =
                                user_data.encoders_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }

                    if user_data.count_props >= count_props {
                        for (i, (id, value)) in conn.properties().enumerate() {
                            let id_offset =
                                user_data.props_ptr as usize + i * core::mem::size_of::<u32>();
                            let value_offset = user_data.prop_values_ptr as usize
                                + i * core::mem::size_of::<u64>();
                            current_userspace!().write_val(id_offset, id)?;
                            current_userspace!().write_val(value_offset, value)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetProperty => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetProperty = cmd.read()?;
                let prop_id = user_data.prop_id;

                let property = match self.device.resources().lock().get_properties(&prop_id) {
                    Some(prop) => prop,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_values = property.count_values();
                let count_enum_blobs = property.count_enum_blobs();

                if user_data.is_first_call() {
                    user_data.name = property.name();
                    user_data.flags = property.flags();
                    user_data.count_values = count_values;
                    user_data.count_enum_blobs = count_enum_blobs;

                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_values < count_values
                        || user_data.count_enum_blobs < count_enum_blobs
                    {
                        return_errno!(Errno::EINVAL);
                    }

                    match property.kind() {
                        PropertyKind::Range { min, max } => {
                            let values = [*min, *max];
                            for (i, val) in values.iter().enumerate() {
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<u64>();
                                current_userspace!().write_val(offset, val)?;
                            }
                        }
                        PropertyKind::SignedRange { min, max } => {
                            let values = [*min, *max];
                            for (i, val) in values.iter().enumerate() {
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<i64>();
                                current_userspace!().write_val(offset, val)?;
                            }
                        }
                        PropertyKind::Enum(items) | PropertyKind::Bitmask(items) => {
                            for (i, (val, name)) in items.iter().enumerate() {
                                // set value
                                let offset =
                                    user_data.values_ptr as usize + i * core::mem::size_of::<u64>();
                                current_userspace!().write_val(offset, val)?;

                                // set enum
                                let prop_enum = PropertyEnum::new(*val, name);
                                let enum_offset = user_data.enum_blob_ptr as usize
                                    + i * core::mem::size_of::<PropertyEnum>();
                                current_userspace!().write_val(enum_offset, &prop_enum)?;
                            }
                        }
                        PropertyKind::Blob(blob_id) => {
                            current_userspace!()
                                .write_val(user_data.values_ptr as usize, blob_id)?;
                        }
                        PropertyKind::Object(obj_type) => {
                            current_userspace!()
                                .write_val(user_data.values_ptr as usize, &(*obj_type as u32))?;
                        }
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeSetProperty => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let _user_data: DrmModeConnectorSetProperty = cmd.read()?;

                // TODO

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPropBlob => {
                // TODO: implement property blob lookup and data copy.
                //
                // In the Linux DRM implementation, MODE_GETPROPBLOB needs to:
                //   * lookup the blob object by id (drm_property_blob_lookup_blob())
                //   * copy the blob data to userspace if the provided buffer is large enough
                //   * update the returned length field to reflect actual blob size
                //
                // This is required to correctly support blob-type properties exposed to userspace (e.g., IN_FORMATS).
                // Currently this is a stub and does not perform any blob resolution or data transfer.
                let _user_data: DrmModeGetBlob = cmd.read()?;
                Ok(0)
            }
            cmd @ DrmIoctlModeAddFB => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeFBCmd = cmd.read()?;
                let handle = user_data.handle;

                if let Some(gem_obj) = self.lookup_gem(&handle) {
                    // TODO: format check && flag check

                    let mut mode_config = self.device.resources().lock();
                    // TODO: the create_framebuffer is provide from
                    // framebuffer.funcs.create()
                    let fb_id = mode_config.create_framebuffer(
                        user_data.width,
                        user_data.height,
                        user_data.pitch,
                        user_data.bpp,
                        gem_obj,
                    );

                    user_data.fb_id = fb_id;

                    cmd.write(&user_data)?;
                } else {
                    return_errno!(Errno::EINVAL)
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeRmFB => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmModeFBCmd = cmd.read()?;
                let fb_id = user_data.fb_id;

                let mut mode_config = self.device.resources().lock();
                let _ = mode_config.remove_framebuffer(&fb_id);

                Ok(0)
            }
            cmd @ DrmIoctlModeDirtyFb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let user_data: DrmModeFbDirtyCmd = cmd.read()?;
                let fb_id = user_data.fb_id;

                // TODO: just legacy achievement
                if let Some(framebuffer) = FRAMEBUFFER.get() {
                    let iomem = framebuffer.io_mem();
                    let mut writer = iomem.writer().to_fallible();

                    let mode_config = self.device.resources().lock();
                    if let Some(drm_framebuffer) = mode_config.lookup_framebuffer(&fb_id) {
                        // TODO: handle the error
                        let _ = drm_framebuffer.read(0, &mut writer);
                    } else {
                        return_errno!(Errno::ENOENT);
                    }
                } else {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeCreateDumb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeCreateDumb = cmd.read()?;

                if let Some(dumb_create) = self.device.driver().driver_ops().dumb_create {
                    // TODO: handle the error
                    if let Ok(gem) = match dumb_create {
                        DumbCreateProvider::Memfd => {
                            dumb_create_impl(user_data.width, user_data.height, user_data.bpp)
                        }
                        DumbCreateProvider::Custom(dumb_create_impl) => {
                            dumb_create_impl(user_data.width, user_data.height, user_data.bpp)
                        }
                    } {
                        let handle = self.next_handle();
                        user_data.handle = handle;
                        user_data.pitch = gem.pitch();
                        user_data.size = gem.size();

                        self.insert_gem(handle, gem);

                        cmd.write(&user_data)?;
                    }
                } else {
                    return_errno!(Errno::ENOENT);
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeMapDumb => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeMapDumb = cmd.read()?;
                let handle = user_data.handle;

                if self.device.driver().driver_ops().dumb_create.is_none() {
                    return_errno!(Errno::ENOSYS);
                }

                if let Some(gem_obj) = self.lookup_gem(&handle) {
                    // TODO: Don't allow imported objects to be mapped
                    user_data.offset = self.device.create_offset(gem_obj);

                    cmd.write(&user_data)?;
                } else {
                    return_errno!(Errno::ENOENT)
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeDestroyDumb => {
                if self.device.driver().driver_ops().dumb_create.is_none() {
                    return_errno!(Errno::ENOSYS);
                }

                let user_data: DrmModeDestroyDumb = cmd.read()?;
                let handle = user_data.handle;

                if let Some(gem_obj) = self.remove_gem(&handle) {
                    // TODO: handle the error
                    let _ = gem_obj.release();
                    self.device.remove_offset(&gem_obj);
                } else {
                    return_errno!(Errno::EINVAL)
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPlaneResources => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetPlaneRes = cmd.read()?;
                let count_planes = self.device.resources().lock().count_planes();

                if user_data.is_first_call() {
                    user_data.count_planes = count_planes;
                    cmd.write(&user_data)?;
                } else {
                    // TODO: apply legacy plane filtering per client capabilities.
                    //
                    // Linux DRM only advertises overlay planes by default for legacy userspace.
                    // If the client has enabled the `DRM_CLIENT_CAP_UNIVERSAL_PLANES` cap (or
                    // supports atomic), primary and cursor planes should also be exposed.
                    // See drm_for_each_plane() and the handling of `file_priv->universal_planes`
                    // in the C implementation.

                    if user_data.count_planes >= count_planes {
                        for (i, id) in self.device.resources().lock().planes_id().enumerate() {
                            let offset =
                                user_data.plane_id_ptr as usize + i * core::mem::size_of::<u32>();
                            current_userspace!().write_val(offset, &id)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeGetPlane => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeGetPlane = cmd.read()?;
                let plane_id = user_data.plane_id;

                let _plane = match self.device.resources().lock().get_plane(&plane_id) {
                    Some(plane) => plane,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                // TODO: support state and format querying per Linux DRM semantics.
                //
                // The Linux DRM GETPLANE ioctl returns a plane’s current state in addition
                // to basic identifiers. In a full implementation, userspace expects:
                //
                //   * CRTC/fb binding from the current atomic or legacy plane state.
                //   * Plane formats and format count via `count_format_types`/`format_type_ptr`.
                //   * Checks for atomic capability and client caps (e.g., DRM_CLIENT_CAP_ATOMIC).
                //
                // At minimum, atomic state lookup must be done to fill `crtc_id`, `fb_id`,
                // and format lists per current plane state. This stub only zeroes gamma_size.

                user_data.gamma_size = 0;
                cmd.write(&user_data)?;

                Ok(0)
            }
            cmd @ DrmIoctlModeObjectGetProps => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let mut user_data: DrmModeObjectGetProps = cmd.read()?;
                let obj_id = user_data.obj_id;

                let obj = match self.device.resources().lock().get_object(&obj_id) {
                    Some(o) => o,
                    None => {
                        return_errno!(Errno::ENOENT);
                    }
                };

                let count_props = obj.count_props();

                if user_data.is_first_call() {
                    user_data.count_props = count_props;
                    cmd.write(&user_data)?;
                } else {
                    if user_data.count_props >= count_props {
                        for (i, (id, value)) in obj.get_properties().enumerate() {
                            let id_offset =
                                user_data.props_ptr as usize + i * core::mem::size_of::<u32>();
                            let value_offset = user_data.prop_values_ptr as usize
                                + i * core::mem::size_of::<u64>();

                            current_userspace!().write_val(id_offset, &id)?;
                            current_userspace!().write_val(value_offset, &value)?;
                        }
                    } else {
                        return_errno!(Errno::EFAULT);
                    }
                }

                Ok(0)
            }
            cmd @ DrmIoctlModeListLessees => {
                if !self.device.check_feature(DrmDriverFeatures::MODESET) {
                    return_errno!(Errno::EOPNOTSUPP);
                }

                let _user_data: DrmModeListLessees = cmd.read()?;

                // TODO:

                Ok(0)
            }
            _ => {
                let driver = self.device.driver();
                match driver.handle_command(raw_ioctl.cmd(), raw_ioctl.arg()) {
                    Ok(()) => Ok(0),
                    Err(()) => {
                        // TODO: handle error
                        log::debug!(
                            "the ioctl command {:#x} is unknown for drm devices",
                            raw_ioctl.cmd()
                        );
                        return_errno_with_message!(Errno::ENOTTY, "the ioctl command is unknown");
                    }
                }
            }
        })
    }
}
