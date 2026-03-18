// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]

mod helper;
mod simpledrm;

use aster_framebuffer::FRAMEBUFFER;
use component::{ComponentInitError, init_component};

extern crate alloc;

#[init_component]
fn sysfb_component_init() -> Result<(), ComponentInitError> {
    // if FRAMEBUFFER.get().is_some() {
    //     simpledrm::register_device();
    // }
    // simpledrm::register_driver();
    Ok(())
}
