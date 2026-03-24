use alloc::{boxed::Box, sync::Arc, vec::Vec};

use hashbrown::{HashMap, HashSet};
use ostd::sync::Mutex;

use crate::drm::{
    DrmError,
    mode_config::{
        DrmModeConfig, DrmModeModeInfo, DrmModeObject, connector::funcs::ConnectorFuncs,
        encoder::DrmEncoder,
    },
};

pub mod funcs;
pub mod property;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum DrmModeConnType {
    Unknown = 0,
    VGA = 1,
    DVII = 2,
    DVID = 3,
    DVIA = 4,
    Composite = 5,
    SVIDEO = 6,
    LVDS = 7,
    Component = 8,
    _9PinDIN = 9,
    DisplayPort = 10,
    HDMIA = 11,
    HDMIB = 12,
    TV = 13,
    eDP = 14,
    VIRTUAL = 15,
    DSI = 16,
    DPI = 17,
    WRITEBACK = 18,
    SPI = 19,
    USB = 20,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum ConnectorStatus {
    // DRM_MODE_CONNECTED
    Connected = 1,
    // DRM_MODE_DISCONNECTED
    Disconnected = 2,
    // DRM_MODE_UNKNOWNCONNECTION
    Unknownconnection = 3,
}

bitflags::bitflags! {
    struct SubpixelOrder: u32 {
        const RGB444    = 1<<0;
        const YCBCR444  = 1<<1;
        const YCBCR422  = 1<<2;
        const YCBCR420  = 1<<3;
    }
}

#[derive(Debug)]
struct DrmDisplayInfo {
    mm_width: u32,
    mm_height: u32,
    subpixel_order: SubpixelOrder,
}

impl DrmDisplayInfo {
    pub fn mm_width(&self) -> u32 {
        self.mm_width
    }

    pub fn mm_height(&self) -> u32 {
        self.mm_height
    }

    pub fn subpixel_order(&self) -> u32 {
        self.subpixel_order.bits()
    }
}

#[derive(Debug)]
pub struct DrmConnector {
    id: u32,
    encoder: Option<u32>,
    modes: Mutex<HashSet<DrmModeModeInfo>>,
    properties: HashMap<u32, u64>,
    possible_encoders_id: HashSet<u32>,
    possible_encoders_mask: u32,

    type_: DrmModeConnType,
    type_id: u32,
    status: Mutex<ConnectorStatus>,

    display_info: Mutex<DrmDisplayInfo>,
    pub funcs: Box<dyn ConnectorFuncs>,
}

impl DrmConnector {
    pub fn init_with_encoder(
        res: &mut DrmModeConfig,
        encoder: &[Arc<DrmEncoder>],
        funcs: Box<dyn ConnectorFuncs>,
    ) -> Result<Arc<Self>, DrmError> {
        let id = res.next_object_id();
        let mut properties = HashMap::new();
        if let Some(prop_id) = res.find_property_id_by_name("DPMS") {
            properties.insert(prop_id, 1);
        }
        if let Some(prop_id) = res.find_property_id_by_name("CRTC_ID") {
            properties.insert(prop_id, 0);
        }
        if let Some(prop_id) = res.find_property_id_by_name("non-desktop") {
            properties.insert(prop_id, 0);
        }

        let mut conn = Self {
            id,
            encoder: None,
            modes: Mutex::new(HashSet::new()),
            properties,
            possible_encoders_id: HashSet::new(),
            possible_encoders_mask: 0,

            type_: DrmModeConnType::Unknown,
            // TODO: auto allocat, not repeat
            type_id: 1,
            status: Mutex::new(ConnectorStatus::Unknownconnection),

            // TODO: use true data
            display_info: Mutex::new(DrmDisplayInfo {
                mm_width: 384,
                mm_height: 240,
                subpixel_order: SubpixelOrder { bits: 0 },
            }),
            funcs,
        };

        encoder.iter().for_each(|e| {
            if conn.encoder.is_none() {
                conn.encoder = Some(e.id());
            }
            conn.possible_encoders_id.insert(e.id());
            conn.possible_encoders_mask |= 1u32 << e.index();
        });

        Ok(Arc::new(conn))
    }

    pub fn attach_property(&mut self, property_id: u32, value: u64) {
        self.properties.insert(property_id, value);
    }

    pub fn type_(&self) -> DrmModeConnType {
        self.type_
    }

    pub fn type_id_(&self) -> u32 {
        self.type_id
    }

    pub fn set_connector_identity(&mut self, type_: DrmModeConnType, type_id: u32) {
        self.type_ = type_;
        self.type_id = type_id;
    }

    pub fn status(&self) -> ConnectorStatus {
        self.status.lock().clone()
    }

    pub fn update_status(&self, status: ConnectorStatus) -> Result<(), DrmError> {
        let mut old_status = self.status.lock();
        *old_status = status;
        Ok(())
    }

    pub fn mm_width(&self) -> u32 {
        self.display_info.lock().mm_width()
    }

    pub fn mm_height(&self) -> u32 {
        self.display_info.lock().mm_height()
    }

    pub fn subpixel_order(&self) -> u32 {
        self.display_info.lock().subpixel_order()
    }

    pub fn update_display_info(
        &self,
        mm_width: u32,
        mm_height: u32,
        subpixel_order_bits: u32,
    ) {
        let mut display_info = self.display_info.lock();
        display_info.mm_width = mm_width;
        display_info.mm_height = mm_height;
        display_info.subpixel_order = SubpixelOrder::from_bits_truncate(subpixel_order_bits);
    }

    pub fn encoder(&self) -> Option<u32> {
        self.encoder
    }

    pub fn modes(&self) -> Vec<DrmModeModeInfo> {
        self.modes.lock().iter().cloned().collect()
    }

    pub fn update_modes(&self, modes: &[DrmModeModeInfo]) -> Result<(), DrmError> {
        let mut old_modes = self.modes.lock();
        old_modes.clear();
        old_modes.extend(modes.iter().cloned());

        Ok(())
    }

    pub fn properties(&self) -> impl Iterator<Item = (&u32, &u64)> {
        self.properties.iter()
    }

    pub fn possible_encoders_id(&self) -> impl Iterator<Item = &u32> {
        self.possible_encoders_id.iter()
    }

    pub fn count_modes(&self) -> u32 {
        self.modes.lock().iter().count() as u32
    }

    pub fn count_props(&self) -> u32 {
        self.properties.iter().count() as u32
    }

    pub fn count_encoders(&self) -> u32 {
        self.possible_encoders_mask.count_ones()
    }
}

impl DrmModeObject for DrmConnector {
    fn id(&self) -> u32 {
        self.id
    }

    fn properties(&self) -> &HashMap<u32, u64> {
        &self.properties
    }
}
