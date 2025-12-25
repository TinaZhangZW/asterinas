// SPDX-License-Identifier: MPL-2.0

use alloc::collections::HashSet;

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

impl Default for ConnectorStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Debug, Default, Clone)]
pub struct Connector {
    pub id: u32,
    pub encoder: u32,
    pub props: HashSet<u32>,
    pub modes: HashSet<super::mode::ModeInfo>,
    pub possible_encoders: HashSet<u32>,
    pub status: ConnectorStatus,
}

impl Connector {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            encoder: Default::default(),
            props: HashSet::new(),
            modes: HashSet::new(),
            possible_encoders: HashSet::new(),
            status: ConnectorStatus::default(),
        }
    }

    pub fn count_modes(&self) -> u32 {
        self.modes.len() as u32
    }

    pub fn count_props(&self) -> u32 {
        self.props.len() as u32
    }

    pub fn count_encoders(&self) -> u32 {
        self.possible_encoders.len() as u32
    }

    pub fn attach_encoder(&mut self, encoder: &super::encoder::ModeEncoder) {
        self.possible_encoders.insert(encoder.id());
    }

    pub fn attach_mode(&mut self, mode: super::mode::ModeInfo) {
        self.modes.insert(mode);
    }

    pub fn attach_properties(&mut self, property: super::property::Property) {
        self.props.insert(property.id());
    }

    pub fn attach_monitor(&mut self) {
        self.status = ConnectorStatus::Connected;
    }
}