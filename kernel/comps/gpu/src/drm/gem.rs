use alloc::sync::Arc;
use core::{any::Any, fmt::Debug};

use ostd::mm::{VmReader, VmWriter};

/// Trait representing a pluggable GEM buffer backend.
///
/// A type implementing `DrmGemBackend` can be used as the storage layer
/// for a `DrmGemObject`. Drivers choose which backend to use; for
/// example, a simple shmem-like backend or a more complex
/// hardware-specific allocator.
pub trait DrmGemBackend: Debug + Any + Sync + Send {
    fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<usize, ()>;
    fn write(&self, offset: usize, reader: &mut VmReader) -> Result<usize, ()>;

    fn release(&self) -> Result<(), ()>;
}

impl dyn DrmGemBackend {
    /// Attempts to downcast the backend to a concrete type.
    pub fn downcast_ref<T: DrmGemBackend>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

/// A GEM buffer object holding metadata and a backend instance.
///
/// A GEM object encapsulates generic buffer metadata (size and pitch) and
/// delegates actual memory access to its backend. This differs from
/// Linux's `drm_gem_object` in that the storage implementation is
/// abstracted behind a trait rather than being tied to shmem or private
/// backing directly. Drivers can plug in any backend implementing the
/// trait to satisfy GEM buffer requirements.
#[derive(Debug)]
pub struct DrmGemObject {
    size: u64,
    pitch: u32,
    backend: Arc<dyn DrmGemBackend>,
}

impl DrmGemObject {
    /// Creates a new GEM object with the provided metadata and backend.
    pub fn new(size: u64, pitch: u32, backend: Arc<dyn DrmGemBackend>) -> Self {
        Self {
            size,
            pitch,
            backend,
        }
    }

    /// Releases backend resources associated with the GEM object.
    pub fn release(&self) -> Result<(), ()> {
        self.backend.release()
    }

    /// Returns the buffer size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the buffer pitch in bytes.
    pub fn pitch(&self) -> u32 {
        self.pitch
    }

    /// Reads data from the buffer into `writer`, starting at `offset`.
    pub fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<usize, ()> {
        self.backend.read(offset, writer)
    }

    /// Writes data into the buffer from `reader`, starting at `offset`.
    pub fn write(&self, offset: usize, reader: &mut VmReader) -> Result<usize, ()> {
        self.backend.write(offset, reader)
    }

    /// Attempts to downcast the backend to a concrete type.
    pub fn downcast_ref<T: DrmGemBackend>(&self) -> Option<&T> {
        self.backend.downcast_ref()
    }
}
