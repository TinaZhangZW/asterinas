use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

use hashbrown::HashMap;

use crate::drm::{
    DrmError,
    mode_config::{DrmModeConfig, DrmModeObject, plane::funcs::PlaneFuncs},
};

pub mod funcs;
pub mod property;

#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum PlaneType {
    Overlay = 0,
    Primary = 1,
    Cursor = 2,
}

#[derive(Debug)]
pub struct DrmPlane {
    id: u32,
    type_: PlaneType,
    fb_id: AtomicU32,
    crtc_id: AtomicU32,
    possible_crtcs: AtomicU32,

    properties: HashMap<u32, u64>,
    funcs: Box<dyn PlaneFuncs>,
}

impl DrmPlane {
    pub fn init(
        res: &mut DrmModeConfig,
        type_: PlaneType,
        funcs: Box<dyn PlaneFuncs>,
    ) -> Result<Arc<Self>, DrmError> {
        let id = res.next_object_id();
        let mut properties = HashMap::new();
        if let Some(prop_id) = res.find_property_id_by_name("type") {
            properties.insert(prop_id, type_ as u64);
        }
        for name in [
            "SRC_X",
            "SRC_Y",
            "SRC_W",
            "SRC_H",
            "CRTC_X",
            "CRTC_Y",
            "CRTC_W",
            "CRTC_H",
            "FB_ID",
            "CRTC_ID",
            "IN_FORMATS",
        ] {
            if let Some(prop_id) = res.find_property_id_by_name(name) {
                properties.insert(prop_id, 0);
            }
        }
        if let Some(prop_id) = res.find_property_id_by_name("IN_FENCE_FD") {
            // Linux default is -1 for IN_FENCE_FD.
            properties.insert(prop_id, u64::MAX);
        }

        let plane = Self {
            id,
            type_,
            fb_id: AtomicU32::new(0),
            crtc_id: AtomicU32::new(0),
            possible_crtcs: AtomicU32::new(0),
            properties,
            funcs,
        };

        let plane = Arc::new(plane);
        res.planes.insert(id, plane.clone());
        res.objects.insert(id, plane.clone());

        Ok(plane)
    }

    pub fn type_(&self) -> PlaneType {
        self.type_
    }
    pub fn fb_id(&self) -> u32 {
        self.fb_id.load(Ordering::Acquire)
    }

    pub fn crtc_id(&self) -> u32 {
        self.crtc_id.load(Ordering::Acquire)
    }

    pub fn possible_crtcs(&self) -> u32 {
        self.possible_crtcs.load(Ordering::Acquire)
    }

    pub fn add_possible_crtc(&self, crtc_index: u8) {
        self.possible_crtcs
            .fetch_or(1u32 << crtc_index, Ordering::AcqRel);
    }

    pub fn set_state(&self, crtc_id: u32, fb_id: u32) {
        self.crtc_id.store(crtc_id, Ordering::Release);
        self.fb_id.store(fb_id, Ordering::Release);
    }
}

impl DrmModeObject for DrmPlane {
    fn id(&self) -> u32 {
        self.id
    }

    fn properties(&self) -> &HashMap<u32, u64> {
        &self.properties
    }
}
