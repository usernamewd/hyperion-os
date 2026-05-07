//! Filesystem-facing types shared with userland.

use bitflags::bitflags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
}

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
    #[derive(Clone, Copy, Debug)]
    pub struct OpenFlags: u32 {
        const READ   = 1 << 0;
        const WRITE  = 1 << 1;
        const CREATE = 1 << 2;
        const APPEND = 1 << 3;
    }
}
