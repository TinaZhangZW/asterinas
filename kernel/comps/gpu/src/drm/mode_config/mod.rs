use alloc::{boxed::Box, sync::Arc};
use core::{
    any::Any,
    fmt::Debug,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

use hashbrown::HashMap;
use ostd::Pod;

use crate::drm::{
    gem::DrmGemObject,
    mode_config::{
        connector::DrmConnector, crtc::DrmCrtc, encoder::DrmEncoder, framebuffer::DrmFramebuffer,
        plane::DrmPlane, property::DrmProperty,
    },
};

pub mod connector;
pub mod crtc;
pub mod encoder;
pub mod framebuffer;
pub mod plane;
pub mod property;

const DRM_DISPLAY_MODE_LEN: usize = 32;

pub trait DrmModeConfigFuncs: Debug + Any + Sync + Send {
    fn fb_create(
        &self,
        mode_config: &mut DrmModeConfig,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
    ) -> u32;
}

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

#[derive(Debug, Default)]
pub struct DrmModeConfig {
    planes: HashMap<u32, Arc<DrmPlane>>,
    crtcs: HashMap<u32, Arc<DrmCrtc>>,
    encoders: HashMap<u32, Arc<DrmEncoder>>,
    connectors: HashMap<u32, Arc<DrmConnector>>,
    framebuffers: HashMap<u32, Arc<DrmFramebuffer>>,

    funcs: Option<Arc<dyn DrmModeConfigFuncs>>,

    next_object_id: AtomicU32,
    objects: HashMap<u32, Arc<dyn DrmModeObject>>,
    next_prop_id: AtomicU32,
    properties: HashMap<u32, Arc<DrmProperty>>,

    crtc_index: AtomicU8,
    encoder_index: AtomicU8,

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
    pub fn default() -> Self {
        Self {
            planes: HashMap::new(),
            crtcs: HashMap::new(),
            encoders: HashMap::new(),
            connectors: HashMap::new(),
            framebuffers: HashMap::new(),

            funcs: None,

            preferred_depth: 16,
            prefer_shadow: 0,

            next_object_id: AtomicU32::new(0),
            objects: HashMap::new(),
            next_prop_id: AtomicU32::new(1),
            properties: HashMap::new(),

            crtc_index: AtomicU8::new(0),
            encoder_index: AtomicU8::new(0),

            min_width: 1,
            max_width: 8192,
            min_height: 1,
            max_height: 8192,

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
        // TODO: iterate over the predefined set of standard properties from object/property.rs
        // and register them in a generic, data-driven way instead of manual insertion.
    }

    pub fn next_object_id(&self) -> u32 {
        self.next_object_id.fetch_add(1, Ordering::SeqCst)
    }
    pub fn next_prop_id(&self) -> u32 {
        self.next_prop_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn create_framebuffer(
        &mut self,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
    ) -> u32 {
        if let Some(funcs) = self.funcs.clone() {
            return funcs.fb_create(self, width, height, pitch, bpp, gem_obj);
        }

        self.create_framebuffer_default(width, height, pitch, bpp, gem_obj)
    }

    pub fn create_framebuffer_default(
        &mut self,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
    ) -> u32 {
        let id = self.next_object_id();
        let fb = Arc::new(DrmFramebuffer::new(id, width, height, pitch, bpp, gem_obj));
        self.framebuffers.insert(id, fb.clone());
        self.objects.insert(id, fb);
        id
    }

    pub fn set_mode_config_funcs(&mut self, funcs: Option<Arc<dyn DrmModeConfigFuncs>>) {
        self.funcs = funcs;
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

    pub fn get_object(&self, id: &u32) -> Option<Arc<dyn DrmModeObject>> {
        self.objects.get(id).cloned()
    }
    pub fn get_properties(&self, id: &u32) -> Option<Arc<DrmProperty>> {
        self.properties.get(id).cloned()
    }
}
