use alloc::sync::Arc;
use core::{any::Any, fmt::Debug};

use ostd::mm::{VmReader, VmWriter};

use crate::drm::DrmError;

/// Trait representing a pluggable GEM buffer backend.
///
/// A type implementing `DrmGemBackend` can be used as the storage layer
/// for a `DrmGemObject`. Drivers choose which backend to use; for
/// example, a simple shmem‑like backend or a more complex
/// hardware‑specific allocator.
pub trait DrmGemBackend: Debug + Any + Sync + Send {
    /// Read data from the buffer into the provided writer.  The writer is
    /// assumed to be fallible, which is the common case for kernel-side
    /// operations; callers with an `Infallible` writer can convert it using
    /// [`VmWriter::to_fallible`].
    fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<usize, DrmError>;

    /// Write data into the buffer from the provided reader.
    fn write(&self, offset: usize, reader: &mut VmReader) -> Result<usize, DrmError>;

    fn release(&self) -> Result<(), DrmError>;
}

impl dyn DrmGemBackend {
    pub fn downcast_ref<T: DrmGemBackend>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

/// A GEM buffer object holding metadata and a backend instance.
///
/// A GEM object encapsulates generic buffer metadata (size and pitch)
/// and delegates actual memory access to its backend. This differs from
/// Linux’s `drm_gem_object` in that the storage implementation is
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
    pub fn new(size: u64, pitch: u32, backend: Arc<dyn DrmGemBackend>) -> Self {
        Self {
            size,
            pitch,
            backend,
        }
    }

    /// Size of the buffer, in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Pitch (bytes per line) as provided when the object was created.
    pub fn pitch(&self) -> u32 {
        self.pitch
    }

    pub fn release(&self) -> Result<(), DrmError> {
        self.backend.release()
    }

    pub fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<usize, DrmError> {
        self.backend.read(offset, writer)
    }

    pub fn write(&self, offset: usize, reader: &mut VmReader) -> Result<usize, DrmError> {
        self.backend.write(offset, reader)
    }

    pub fn downcast_ref<T: DrmGemBackend>(&self) -> Option<&T> {
        self.backend.downcast_ref()
    }
}
