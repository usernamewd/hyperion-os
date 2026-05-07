//! Inode types.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
}

/// An inode is the on-medium representation of a file/dir. Concrete
/// filesystems implement this trait. Inode references are reference-counted
/// because the same inode may be open from multiple handles.
pub trait Inode: Send + Sync {
    fn ftype(&self) -> FileType;
    fn name(&self) -> String;
    fn size(&self) -> usize;
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, super::FsError>;
    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize, super::FsError>;
    fn list(&self) -> Vec<String>;
    fn lookup(&self, name: &str) -> Option<InodeRef>;
    fn create_file(&self, name: &str, contents: &[u8]) -> Result<InodeRef, super::FsError>;
    fn create_dir(&self, name: &str) -> Result<InodeRef, super::FsError>;
    fn remove(&self, name: &str) -> Result<(), super::FsError>;
}

/// Shared, mutable inode reference.
pub type InodeRef = Arc<dyn Inode>;
