use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::Ordering;

use hashbrown::HashMap;

use crate::drm::{
    DrmError,
    mode_config::{DrmModeConfig, DrmModeObject, crtc::DrmCrtc, encoder::funcs::EncoderFuncs},
};

pub mod funcs;
pub mod property;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum EncoderType {
    None = 0,
    DAC = 1,
    TMDS = 2,
    LVDS = 3,
    TVDAC = 4,
    VIRTUAL = 5,
    DSI = 6,
    DPMST = 7,
    DPI = 8,
}

#[derive(Debug)]
pub struct DrmEncoder {
    id: u32,
    type_: EncoderType,
    index: u8,
    crtc: Option<u32>,

    properties: HashMap<u32, u64>,

    possible_crtcs: u32,
    possible_clones: u32,

    funcs: Box<dyn EncoderFuncs>,
}

impl DrmEncoder {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn init_with_crtcs(
        res: &mut DrmModeConfig,
        type_: EncoderType,
        crtcs: &[Arc<DrmCrtc>],
        funcs: Box<dyn EncoderFuncs>,
    ) -> Result<Arc<Self>, DrmError> {
        let id = res.next_object_id();
        let mut encoder = Self {
            id,
            type_,
            index: res.encoder_index.fetch_add(1, Ordering::SeqCst),
            crtc: None,
            properties: HashMap::new(),
            possible_crtcs: 0,
            possible_clones: 0,
            funcs,
        };

        crtcs.iter().for_each(|c| {
            if encoder.crtc.is_none() {
                // Keep a current CRTC for legacy GETENCODER userspace paths.
                encoder.crtc = Some(c.id());
            }
            encoder.possible_crtcs |= 1u32 << c.index();
        });

        let encoder = Arc::new(encoder);
        res.encoders.insert(id, encoder.clone());
        res.objects.insert(id, encoder.clone());

        Ok(encoder)
    }

    pub fn type_(&self) -> EncoderType {
        self.type_
    }

    pub fn possible_crtcs(&self) -> u32 {
        self.possible_crtcs
    }

    pub fn possible_clones(&self) -> u32 {
        self.possible_clones
    }

    pub fn crtc_id(&self) -> u32 {
        self.crtc.unwrap_or(0)
    }
}

impl DrmModeObject for DrmEncoder {
    fn id(&self) -> u32 {
        self.id
    }

    fn properties(&self) -> &HashMap<u32, u64> {
        &self.properties
    }
}
