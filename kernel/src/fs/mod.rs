//! Virtual filesystem layer.
//!
//! Hyperion follows the Unix tradition of a unified namespace: a tree of
//! [`Inode`]s rooted at "/". The default mount is an in-memory [`ramfs`].
//! New filesystems plug in by providing a [`Filesystem`] impl.
//!
//! The API is intentionally small and synchronous; async I/O can layer on
//! top through user-space services that call into [`open`] / [`read`] /
//! [`write`].

pub mod inode;
pub mod ramfs;
pub mod vfs;

pub use inode::{FileType, Inode, InodeRef};
pub use vfs::{File, FsError, OpenFlags, ROOT};

/// One-time FS init: mount a ramfs at "/" and seed it with a few useful
/// pseudo-files so the shell has something to show.
pub fn init() {
    vfs::init();
    // Seed: /welcome, /version, /dev, /tmp.
    ROOT.create_file("welcome", b"Welcome to Hyperion OS!\r\nTry: help\r\n")
        .ok();
    ROOT.create_file("version", format_version().as_bytes())
        .ok();
    ROOT.create_dir("dev").ok();
    ROOT.create_dir("tmp").ok();
}

fn format_version() -> alloc::string::String {
    use alloc::string::String;
    let mut s = String::new();
    s.push_str("hyperion-os ");
    s.push_str(env!("CARGO_PKG_VERSION"));
    s.push(' ');
    s.push_str(crate::ARCH_NAME);
    s.push_str("\r\n");
    s
}
