// SPDX-License-Identifier: MPL-2.0

use alloc::boxed::Box;
use core::any::Any;
use core::fmt::Debug;
use ostd::sync::Mutex;

#[derive(Debug)]
struct CrtcState {
    // atomic state
}

#[derive(Debug)]
pub struct Crtc {
    id: u32,
    state: Mutex<CrtcState>,
    funcs: Box<dyn CrtcFuncs>,
}

impl Crtc {
    pub fn new(id: u32, funcs: Box<dyn CrtcFuncs>) -> Self {
        Self {
            id,
            state: Mutex::new(CrtcState {}),
            funcs,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn cursor_set(&self) -> Result<()> {
        self.funcs.cursor_set2()
    }

    pub fn cursor_move(&self) -> Result<()> {
        self.funcs.cursor_move()
    }
}

pub trait CrtcFuncs: Debug + Any + Sync + Send {
    fn cursor_set(&self) -> Result<()> {
        return_errno!(Errno::ENXIO);
    }

    fn cursor_set2(&self) -> Result<()> {
        // fallback to cursor_set
        // same result as Linux
        self.cursor_set()
    }

    fn cursor_move(&self) -> Result<()> {
        return_errno!(Errno::EFAULT);
    }
}