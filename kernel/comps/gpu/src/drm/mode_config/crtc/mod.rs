use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
};
use core::sync::atomic::Ordering;

use hashbrown::HashMap;
use ostd::sync::Mutex;

use crate::drm::{
    DrmError,
    mode_config::{DrmModeConfig, DrmModeObject, crtc::funcs::CrtcFuncs, plane::DrmPlane},
    vblank::DrmVblankState,
};

pub mod funcs;
pub mod helper;
pub mod property;

#[derive(Debug)]
pub struct DrmCrtc {
    id: u32,
    /// human readable name, can be overwritten by the driver
    name: String,

    index: u8,

    properties: HashMap<u32, u64>,
    gamma_size: u32,
    primary_plane: Arc<DrmPlane>,
    cursor_plane: Option<Arc<DrmPlane>>,

    enabled: bool,

    /// xy position on screen.
    x: u32,
    y: u32,

    /// Vblank state (always initialized, but may be disabled)
    /// Even if vblank is not actively used, the state exists
    vblank: Arc<Mutex<DrmVblankState>>,

    pub funcs: Box<dyn CrtcFuncs>,
}

impl DrmCrtc {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn xy(&self) -> (u32, u32) {
        // TODO: use plane state x,y
        (self.x, self.y)
    }

    pub fn fb_id(&self) -> u32 {
        // TODO: fallback to self.fb_id
        self.primary_plane.fb_id()
    }

    pub fn gamma_size(&self) -> u32 {
        self.gamma_size
    }

    pub fn update_primary_plane_state(&self, fb_id: u32) {
        self.primary_plane.set_state(self.id, fb_id);
    }

    /// Get vblank state
    ///
    /// Returns Arc<Mutex<>> which allows access even through Arc<DrmCrtc>
    pub fn vblank_state(&self) -> Arc<Mutex<DrmVblankState>> {
        self.vblank.clone()
    }

    pub fn init_with_planes(
        res: &mut DrmModeConfig,
        name: Option<&str>,
        primary_plane: Arc<DrmPlane>,
        cursor_plane: Option<Arc<DrmPlane>>,
        funcs: Box<dyn CrtcFuncs>,
    ) -> Result<Arc<Self>, DrmError> {
        let id = res.next_object_id();
        let name = match name {
            Some(name) => name.to_string(),
            None => format!("crtc-{}", id),
        };

        let mut properties = HashMap::new();
        if let Some(prop_id) = res.find_property_id_by_name("MODE_ID") {
            properties.insert(prop_id, 0);
        }
        if let Some(prop_id) = res.find_property_id_by_name("ACTIVE") {
            properties.insert(prop_id, 0);
        }
        if let Some(prop_id) = res.find_property_id_by_name("OUT_FENCE_PTR") {
            properties.insert(prop_id, 0);
        }
        if let Some(prop_id) = res.find_property_id_by_name("VRR_ENABLED") {
            properties.insert(prop_id, 0);
        }

        let crtc = Self {
            id,
            name,
            index: res.crtc_index.fetch_add(1, Ordering::SeqCst),
            properties,
            gamma_size: 0,
            primary_plane,
            cursor_plane,
            enabled: false,
            x: 0,
            y: 0,
            vblank: Arc::new(Mutex::new(DrmVblankState::new())),
            funcs,
        };

        // Plane-to-CRTC compatibility masks are consumed by GETPLANE.
        crtc.primary_plane.add_possible_crtc(crtc.index);
        if let Some(cursor_plane) = crtc.cursor_plane.as_ref() {
            cursor_plane.add_possible_crtc(crtc.index);
        }

        // TODO: get x, y, gamma_size

        let crtc = Arc::new(crtc);
        res.crtcs.insert(id, crtc.clone());
        res.objects.insert(id, crtc.clone());

        Ok(crtc)
    }
}

impl DrmModeObject for DrmCrtc {
    fn id(&self) -> u32 {
        self.id
    }

    fn properties(&self) -> &HashMap<u32, u64> {
        &self.properties
    }
}
