use alloc::{boxed::Box, sync::Arc};
use core::{
    any::Any,
    fmt::Debug,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

use hashbrown::HashMap;
use ostd::Pod;

use crate::drm::{
    DrmError,
    gem::DrmGemObject,
    mode_config::{
        connector::DrmConnector, crtc::DrmCrtc, encoder::DrmEncoder, framebuffer::DrmFramebuffer,
        funcs::ModeConfigFuncs, plane::DrmPlane,
        property::{DrmModeObjectType, DrmProperty, PropertyFlags},
    },
};

pub mod connector;
pub mod crtc;
pub mod encoder;
pub mod framebuffer;
pub mod funcs;
pub mod plane;
pub mod property;

const DRM_DISPLAY_MODE_LEN: usize = 32;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, Hash, Eq, PartialEq, Pod)]
pub struct DrmModeModeInfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,

    pub vrefresh: u32,

    pub flags: u32,
    pub type_: u32,

    pub name: [u8; DRM_DISPLAY_MODE_LEN],
}

impl DrmModeModeInfo {
    pub fn new() -> Self {
        todo!()
    }
}

/// DrmModeObject
pub trait DrmModeObject: Debug + Any + Sync + Send {
    fn id(&self) -> u32;

    fn properties(&self) -> &HashMap<u32, u64>;

    fn count_props(&self) -> u32 {
        self.properties().iter().count() as u32
    }

    fn get_properties(&self) -> Box<dyn Iterator<Item = (u32, u64)> + '_> {
        Box::new(self.properties().iter().map(|(&id, &val)| (id, val)))
    }
}

#[derive(Debug)]
pub struct DrmModeConfig {
    planes: HashMap<u32, Arc<DrmPlane>>,
    crtcs: HashMap<u32, Arc<DrmCrtc>>,
    encoders: HashMap<u32, Arc<DrmEncoder>>,
    connectors: HashMap<u32, Arc<DrmConnector>>,
    framebuffers: HashMap<u32, Arc<DrmFramebuffer>>,

    next_object_id: AtomicU32,
    objects: HashMap<u32, Arc<dyn DrmModeObject>>,
    next_prop_id: AtomicU32,
    properties: HashMap<u32, Arc<DrmProperty>>,
    next_blob_id: AtomicU32,
    blobs: HashMap<u32, Arc<[u8]>>,

    crtc_index: AtomicU8,
    encoder_index: AtomicU8,

    funcs: Box<dyn ModeConfigFuncs>,

    pub preferred_depth: u32,
    pub prefer_shadow: u32,

    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,

    pub cursor_width: u32,
    pub cursor_height: u32,

    pub fb_modifiers_not_supported: bool,
    pub async_page_flip: bool,
}

impl DrmModeConfig {
    pub fn new(
        min_width: u32,
        max_width: u32,
        min_height: u32,
        max_height: u32,
        preferred_depth: u32,
        funcs: Box<dyn ModeConfigFuncs>,
    ) -> Self {
        Self {
            planes: HashMap::new(),
            crtcs: HashMap::new(),
            encoders: HashMap::new(),
            connectors: HashMap::new(),
            framebuffers: HashMap::new(),

            preferred_depth,
            prefer_shadow: 0,

            next_object_id: AtomicU32::new(1),
            objects: HashMap::new(),
            next_prop_id: AtomicU32::new(1),
            properties: HashMap::new(),
            next_blob_id: AtomicU32::new(1),
            blobs: HashMap::new(),

            crtc_index: AtomicU8::new(0),
            encoder_index: AtomicU8::new(0),

            funcs,

            min_width,
            max_width,
            min_height,
            max_height,

            cursor_width: 32,
            cursor_height: 32,

            fb_modifiers_not_supported: true,
            async_page_flip: false,
        }
    }

    /// Initialize standard, driver-independent DRM properties for this device.
    ///
    /// In the Linux DRM design, a driver is responsible for registering a set of
    /// generic (standardized) properties during device initialization. These
    /// properties are shared across multiple DRM object types (e.g. connectors,
    /// CRTCs, planes) and define common, well-known behavior expected by
    /// userspace (such as DPMS, link status, scaling mode, etc.).
    ///
    /// This function should be called once during device bring-up, before any
    /// DRM objects are exposed to userspace. Individual DRM objects will later
    /// reference these pre-registered properties when they are created.
    ///
    /// Object-specific or driver-private properties must NOT be registered here;
    /// they should be added during the corresponding object initialization.
    pub fn init_standard_properties(&mut self) {
        if !self.properties.is_empty() {
            return;
        }

        // Connector standard properties.
        self.create_property(DrmProperty::create_blob("EDID", PropertyFlags::IMMUTABLE));
        self.create_property(DrmProperty::create_enum(
            "DPMS",
            PropertyFlags::empty(),
            &[(0, "Off"), (1, "On"), (2, "Standby"), (3, "Suspend")],
        ));
        self.create_property(DrmProperty::create_object(
            "CRTC_ID",
            PropertyFlags::ATOMIC,
            DrmModeObjectType::Crtc,
        ));
        self.create_property(DrmProperty::create_bool(
            "non-desktop",
            PropertyFlags::IMMUTABLE,
        ));

        // Plane standard properties.
        self.create_property(DrmProperty::create_enum(
            "type",
            PropertyFlags::IMMUTABLE,
            &[(0, "Overlay"), (1, "Primary"), (2, "Cursor")],
        ));
        self.create_property(DrmProperty::create_range("SRC_X", PropertyFlags::ATOMIC, 0, u32::MAX as u64));
        self.create_property(DrmProperty::create_range("SRC_Y", PropertyFlags::ATOMIC, 0, u32::MAX as u64));
        self.create_property(DrmProperty::create_range("SRC_W", PropertyFlags::ATOMIC, 0, u32::MAX as u64));
        self.create_property(DrmProperty::create_range("SRC_H", PropertyFlags::ATOMIC, 0, u32::MAX as u64));
        self.create_property(DrmProperty::create_signed_range(
            "CRTC_X",
            PropertyFlags::ATOMIC,
            i32::MIN as i64,
            i32::MAX as i64,
        ));
        self.create_property(DrmProperty::create_signed_range(
            "CRTC_Y",
            PropertyFlags::ATOMIC,
            i32::MIN as i64,
            i32::MAX as i64,
        ));
        self.create_property(DrmProperty::create_range("CRTC_W", PropertyFlags::ATOMIC, 0, i32::MAX as u64));
        self.create_property(DrmProperty::create_range("CRTC_H", PropertyFlags::ATOMIC, 0, i32::MAX as u64));
        self.create_property(DrmProperty::create_object(
            "FB_ID",
            PropertyFlags::ATOMIC,
            DrmModeObjectType::FB,
        ));
        self.create_property(DrmProperty::create_signed_range(
            "IN_FENCE_FD",
            PropertyFlags::ATOMIC,
            -1,
            i32::MAX as i64,
        ));
        self.create_property(DrmProperty::create_range(
            "OUT_FENCE_PTR",
            PropertyFlags::ATOMIC,
            0,
            u64::MAX,
        ));
        self.create_property(DrmProperty::create(
            "FB_DAMAGE_CLIPS",
            PropertyFlags::ATOMIC,
        ));
        self.create_property(DrmProperty::create(
            "IN_FORMATS",
            PropertyFlags::IMMUTABLE,
        ));

        // CRTC standard properties.
        self.create_property(DrmProperty::create_bool("ACTIVE", PropertyFlags::ATOMIC));
        self.create_property(DrmProperty::create("MODE_ID", PropertyFlags::ATOMIC));
        self.create_property(DrmProperty::create("VRR_ENABLED", PropertyFlags::empty()));
    }

    pub fn next_object_id(&self) -> u32 {
        self.next_object_id.fetch_add(1, Ordering::SeqCst)
    }
    pub fn next_prop_id(&self) -> u32 {
        self.next_prop_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn next_blob_id(&self) -> u32 {
        self.next_blob_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn create_property(&mut self, property: DrmProperty) -> u32 {
        let id = self.next_prop_id();
        self.properties.insert(id, Arc::new(property));
        id
    }

    pub fn find_property_id_by_name(&self, name: &str) -> Option<u32> {
        self.properties
            .iter()
            .find_map(|(id, prop)| prop.has_name(name).then_some(*id))
    }

    pub fn create_blob(&mut self, data: Arc<[u8]>) -> u32 {
        let id = self.next_blob_id();
        self.blobs.insert(id, data);
        id
    }

    pub fn get_blob(&self, id: &u32) -> Option<Arc<[u8]>> {
        self.blobs.get(id).cloned()
    }

    pub fn remove_blob(&mut self, id: &u32) -> Option<Arc<[u8]>> {
        self.blobs.remove(id)
    }

    pub fn create_framebuffer(
        &mut self,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
    ) -> Result<u32, DrmError> {
        let mut fb = self
            .funcs
            .create_framebuffer(width, height, pitch, bpp, gem_obj)?;

        // TODO: better way?
        let id = self.next_object_id();
        fb.init_object(id);

        let fb = Arc::new(fb);

        self.framebuffers.insert(id, fb.clone());
        self.objects.insert(id, fb);
        Ok(id)
    }

    pub fn lookup_framebuffer(&self, fb_id: &u32) -> Option<Arc<DrmFramebuffer>> {
        self.framebuffers.get(fb_id).cloned()
    }

    pub fn remove_framebuffer(&mut self, fb_id: &u32) -> Option<Arc<DrmFramebuffer>> {
        self.framebuffers.remove(fb_id)
    }

    pub fn count_planes(&self) -> u32 {
        self.planes.iter().count() as u32
    }
    pub fn count_crtcs(&self) -> u32 {
        self.crtcs.iter().count() as u32
    }
    pub fn count_encoders(&self) -> u32 {
        self.encoders.iter().count() as u32
    }
    pub fn count_connectors(&self) -> u32 {
        self.connectors.iter().count() as u32
    }
    pub fn count_framebuffers(&self) -> u32 {
        self.framebuffers.iter().count() as u32
    }

    pub fn planes_id(&self) -> impl Iterator<Item = u32> + '_ {
        self.planes.keys().copied()
    }
    pub fn crtcs_id(&self) -> impl Iterator<Item = u32> + '_ {
        self.crtcs.keys().copied()
    }
    pub fn encoders_id(&self) -> impl Iterator<Item = u32> + '_ {
        self.encoders.keys().copied()
    }
    pub fn connectors_id(&self) -> impl Iterator<Item = u32> + '_ {
        self.connectors.keys().copied()
    }
    pub fn framebuffer_id(&self) -> impl Iterator<Item = u32> + '_ {
        self.framebuffers.keys().copied()
    }

    pub fn get_plane(&self, id: &u32) -> Option<Arc<DrmPlane>> {
        self.planes.get(id).cloned()
    }
    pub fn get_crtc(&self, id: &u32) -> Option<Arc<DrmCrtc>> {
        self.crtcs.get(id).cloned()
    }
    pub fn get_encoder(&self, id: &u32) -> Option<Arc<DrmEncoder>> {
        self.encoders.get(id).cloned()
    }
    pub fn get_connector(&self, id: &u32) -> Option<Arc<DrmConnector>> {
        self.connectors.get(id).cloned()
    }

    pub fn register_connector(&mut self, connector: Arc<DrmConnector>) {
        let id = connector.id();
        self.connectors.insert(id, connector.clone());
        self.objects.insert(id, connector);
    }

    pub fn get_object(&self, id: &u32) -> Option<Arc<dyn DrmModeObject>> {
        self.objects.get(id).cloned()
    }
    pub fn get_properties(&self, id: &u32) -> Option<Arc<DrmProperty>> {
        self.properties.get(id).cloned()
    }
}
