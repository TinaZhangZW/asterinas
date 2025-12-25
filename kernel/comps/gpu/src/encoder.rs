// SPDX-License-Identifier: MPL-2.0

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum ModeEncoderType {
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

impl Default for ModeEncoderType {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug)]
pub struct ModeEncoder {
    id: u32,
    pub type_: ModeEncoderType,
    crtc: u32,
    pub possible_crtcs: u32,
    pub possible_clone: u32,
}

impl ModeEncoder {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            type_: ModeEncoderType::default(),
            crtc: Default::default(),
            possible_crtcs: Default::default(),
            possible_clone: Default::default(),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn set_possible_crtcs(&mut self, id: u32) {
        self.possible_crtcs |= 1 << id;
    }
}