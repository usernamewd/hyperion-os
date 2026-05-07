//! In-memory filesystem.
//!
//! Used as the default mount at "/". Implements the [`Inode`] trait with a
//! pair of variants for files (byte buffer) and directories (name -> inode
//! map). All state is behind a `Mutex` for interior mutability — the
//! `Inode` trait takes `&self` so that filesystems can be shared via
//! `Arc<dyn Inode>` without exterior locking.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::inode::{FileType, Inode, InodeRef};
use super::vfs::FsError;
use crate::sync::Mutex;

enum NodeData {
    File(Vec<u8>),
    Dir(BTreeMap<String, InodeRef>),
}

pub struct RamNode {
    name: Mutex<String>,
    data: Mutex<NodeData>,
}

impl RamNode {
    pub fn new_file(name: &str, contents: &[u8]) -> Arc<Self> {
        Arc::new(Self {
            name: Mutex::new(name.to_string()),
            data: Mutex::new(NodeData::File(contents.to_vec())),
        })
    }

    pub fn new_dir(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: Mutex::new(name.to_string()),
            data: Mutex::new(NodeData::Dir(BTreeMap::new())),
        })
    }
}

impl Inode for RamNode {
    fn ftype(&self) -> FileType {
        match &*self.data.lock() {
            NodeData::File(_) => FileType::Regular,
            NodeData::Dir(_) => FileType::Directory,
        }
    }

    fn name(&self) -> String {
        self.name.lock().clone()
    }

    fn size(&self) -> usize {
        match &*self.data.lock() {
            NodeData::File(v) => v.len(),
            NodeData::Dir(m) => m.len(),
        }
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        match &*self.data.lock() {
            NodeData::File(v) => {
                if offset >= v.len() {
                    return Ok(0);
                }
                let n = (v.len() - offset).min(buf.len());
                buf[..n].copy_from_slice(&v[offset..offset + n]);
                Ok(n)
            }
            NodeData::Dir(_) => Err(FsError::Io),
        }
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
        let mut g = self.data.lock();
        match &mut *g {
            NodeData::File(v) => {
                if offset + buf.len() > v.len() {
                    v.resize(offset + buf.len(), 0);
                }
                v[offset..offset + buf.len()].copy_from_slice(buf);
                Ok(buf.len())
            }
            NodeData::Dir(_) => Err(FsError::Io),
        }
    }

    fn list(&self) -> Vec<String> {
        match &*self.data.lock() {
            NodeData::Dir(m) => m.keys().cloned().collect(),
            NodeData::File(_) => Vec::new(),
        }
    }

    fn lookup(&self, name: &str) -> Option<InodeRef> {
        match &*self.data.lock() {
            NodeData::Dir(m) => m.get(name).cloned(),
            NodeData::File(_) => None,
        }
    }

    fn create_file(&self, name: &str, contents: &[u8]) -> Result<InodeRef, FsError> {
        let mut g = self.data.lock();
        match &mut *g {
            NodeData::Dir(m) => {
                if m.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }
                let n: InodeRef = RamNode::new_file(name, contents);
                m.insert(name.to_string(), n.clone());
                Ok(n)
            }
            NodeData::File(_) => Err(FsError::NotADirectory),
        }
    }

    fn create_dir(&self, name: &str) -> Result<InodeRef, FsError> {
        let mut g = self.data.lock();
        match &mut *g {
            NodeData::Dir(m) => {
                if m.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }
                let n: InodeRef = RamNode::new_dir(name);
                m.insert(name.to_string(), n.clone());
                Ok(n)
            }
            NodeData::File(_) => Err(FsError::NotADirectory),
        }
    }

    fn remove(&self, name: &str) -> Result<(), FsError> {
        let mut g = self.data.lock();
        match &mut *g {
            NodeData::Dir(m) => {
                let entry = m.get(name).ok_or(FsError::NotFound)?;
                // Refuse to remove a non-empty directory. We probe via the
                // public Inode trait so this is filesystem-agnostic.
                if entry.ftype() == FileType::Directory && !entry.list().is_empty() {
                    return Err(FsError::NotEmpty);
                }
                m.remove(name);
                Ok(())
            }
            NodeData::File(_) => Err(FsError::NotADirectory),
        }
    }
}

/// Construct a fresh ramfs root.
pub fn new_root() -> InodeRef {
    RamNode::new_dir("")
}
