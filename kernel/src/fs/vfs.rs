//! VFS top-level: root mount + path resolution + open/read/write.

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::bitflags;

use super::inode::{FileType, InodeRef};
use super::ramfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotADirectory,
    AlreadyExists,
    InvalidPath,
    Io,
    NotEmpty,
}

bitflags! {
    /// Flags accepted by [`open`].
    #[derive(Clone, Copy, Debug)]
    pub struct OpenFlags: u32 {
        const READ   = 1 << 0;
        const WRITE  = 1 << 1;
        const CREATE = 1 << 2;
        const APPEND = 1 << 3;
    }
}

/// An open file handle.
pub struct File {
    pub inode: InodeRef,
    pub offset: usize,
    pub flags: OpenFlags,
}

impl File {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        if !self.flags.contains(OpenFlags::READ) {
            return Err(FsError::Io);
        }
        let n = self.inode.read_at(self.offset, buf)?;
        self.offset += n;
        Ok(n)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FsError> {
        if !self.flags.contains(OpenFlags::WRITE) {
            return Err(FsError::Io);
        }
        if self.flags.contains(OpenFlags::APPEND) {
            self.offset = self.inode.size();
        }
        let n = self.inode.write_at(self.offset, buf)?;
        self.offset += n;
        Ok(n)
    }

    pub fn seek(&mut self, off: usize) {
        self.offset = off;
    }
}

/// Global root of the unified filesystem namespace.
pub static ROOT: Root = Root {
    mount: spin::Once::new(),
};

pub struct Root {
    mount: spin::Once<InodeRef>,
}

impl Root {
    fn mount_inode(&self) -> &InodeRef {
        self.mount.get().expect("vfs not initialised")
    }

    /// Resolve a "/foo/bar" path to an inode.
    pub fn resolve(&self, path: &str) -> Result<InodeRef, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let path = path.strip_prefix('/').unwrap_or(path);
        let mut cur = Arc::clone(self.mount_inode());
        if path.is_empty() {
            return Ok(cur);
        }
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            if cur.ftype() != FileType::Directory {
                return Err(FsError::NotADirectory);
            }
            cur = cur.lookup(comp).ok_or(FsError::NotFound)?;
        }
        Ok(cur)
    }

    pub fn list_root(&self) -> Vec<String> {
        self.mount_inode().list()
    }

    /// Convenience: create a file under "/".
    pub fn create_file(&self, name: &str, contents: &[u8]) -> Result<InodeRef, FsError> {
        self.mount_inode().create_file(name, contents)
    }

    /// Convenience: create a directory under "/".
    pub fn create_dir(&self, name: &str) -> Result<InodeRef, FsError> {
        self.mount_inode().create_dir(name)
    }

    /// Open a path for the given flags.
    pub fn open(&self, path: &str, flags: OpenFlags) -> Result<File, FsError> {
        let res = self.resolve(path);
        let inode = match res {
            Ok(i) => i,
            Err(FsError::NotFound) if flags.contains(OpenFlags::CREATE) => {
                let (parent_path, name) = split_parent(path);
                let parent = self.resolve(parent_path)?;
                parent.create_file(name, &[])?
            }
            Err(e) => return Err(e),
        };
        Ok(File {
            inode,
            offset: 0,
            flags,
        })
    }

    /// Remove a file or empty directory.
    pub fn remove(&self, path: &str) -> Result<(), FsError> {
        let (parent_path, name) = split_parent(path);
        let parent = self.resolve(parent_path)?;
        parent.remove(name)
    }
}

fn split_parent(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some(("", n)) => ("/", n),
        Some((p, n)) => (p, n),
        None => ("/", path),
    }
}

/// One-time VFS init: mount a ramfs at "/".
pub fn init() {
    ROOT.mount.call_once(|| ramfs::new_root() as InodeRef);
}

/// Read a path as a `String` (helpful for shell `cat`).
pub fn read_to_string(path: &str) -> Result<String, FsError> {
    let mut f = ROOT.open(path, OpenFlags::READ)?;
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        match f.read(&mut chunk)? {
            0 => break,
            n => out.extend_from_slice(&chunk[..n]),
        }
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}
