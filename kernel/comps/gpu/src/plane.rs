// SPDX-License-Identifier: MPL-2.0

use ostd::sync::Mutex;

#[derive(Debug)]
struct PlaneState {
    // atomic state
}

#[derive(Debug)]
pub struct Plane {
    id: u32,
    state: Mutex<PlaneState>,
}

impl Plane {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            state: Mutex::new(PlaneState {}),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}