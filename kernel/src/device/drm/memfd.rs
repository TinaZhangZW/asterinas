use alloc::{format, sync::Arc};

use aster_gpu::drm::{DrmError, gem::DrmGemBackend};
use ostd::mm::{VmReader, VmWriter};

use crate::fs::{
    file_handle::{FileLike, Mappable},
    ramfs::memfd::{MemfdFile, MemfdFlags},
    utils::FallocMode,
};

/// This type wraps a `MemfdFile` as a GEM buffer backend suitable for
/// drivers that use GEM to manage buffer objects. In Linux DRM, GEM
/// objects are abstract buffer objects backed by anonymous memory (often
/// via the shmem filesystem) that drivers expose to userspace for scanout
/// and other operations.
///
/// `DrmMemfdFile` implements the `DrmGemBackend` trait, providing
/// read/write and release callbacks to satisfy GEM’s buffer operations.
/// This can be used in simple or virtual drivers where a generic,
/// pageable memory backend is sufficient (similar in role to a shmem
/// GEM object). It is analogous to Linux drivers using `drm_gem_object_init`
/// with shmem backing, with memfd representing the underlying file.
#[derive(Debug)]
pub struct DrmMemfdFile(MemfdFile);

impl DrmMemfdFile {
    pub fn new(name: &str, size: usize) -> crate::prelude::Result<Arc<dyn DrmGemBackend>> {
        let name = format!("/gem:{}", name);
        let memfd = MemfdFile::new(&name, MemfdFlags::MFD_ALLOW_SEALING)?;
        memfd.fallocate(FallocMode::Allocate, 0, size)?;

        Ok(Arc::new(DrmMemfdFile(memfd)))
    }

    pub fn mappable(&self) -> crate::prelude::Result<Mappable> {
        self.0.mappable()
    }
}

// TODO: How to convert Error to DrmError? Is this nesseary?
impl DrmGemBackend for DrmMemfdFile {
    fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<usize, DrmError> {
        self.0
            .read_at(offset, writer)
            .map_err(|_| DrmError::Invalid)
    }

    fn write(&self, offset: usize, reader: &mut VmReader) -> Result<usize, DrmError> {
        self.0
            .write_at(offset, reader)
            .map_err(|_| DrmError::Invalid)
    }

    fn release(&self) -> Result<(), DrmError> {
        Ok(())
    }
}

pub fn memfd_object_create(name: &str, size: u64) -> Result<Arc<dyn DrmGemBackend>, DrmError> {
    let size_usize = usize::try_from(size).map_err(|_| DrmError::Invalid)?;

    const MAX_GEM_SIZE: usize = 256 * 1024 * 1024; // 256MB
    if size_usize > MAX_GEM_SIZE {
        return Err(DrmError::Invalid);
    }

    DrmMemfdFile::new(name, size_usize).map_err(|_| DrmError::Invalid)
}
