//! Monitors: physical or virtual display endpoints.

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

use super::framebuffer::{Framebuffer, PixelFormat};
use crate::sync::Mutex;

/// Monotonically-assigned monitor identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorKind {
    /// Backed by real display hardware (virtio-gpu, HDMI, DSI, etc.).
    Physical,
    /// Lives entirely in RAM. The compositor still draws to it; clients
    /// can read the framebuffer for screenshots, recording, or sending it
    /// over a remote protocol.
    Virtual,
}

/// A monitor: name, kind, framebuffer, and ID assigned at registration
/// time. The framebuffer is wrapped in a Mutex so the compositor and
/// drawers can share it.
pub struct Monitor {
    pub name: String,
    pub kind: MonitorKind,
    pub width: u32,
    pub height: u32,
    fb: Mutex<Framebuffer>,
    id: AtomicU32,
}

impl Monitor {
    /// Construct a virtual monitor with the given resolution.
    pub fn new_virtual(name: &str, width: u32, height: u32) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            kind: MonitorKind::Virtual,
            width,
            height,
            fb: Mutex::new(Framebuffer::new(width, height, PixelFormat::Bgra8)),
            id: AtomicU32::new(u32::MAX),
        })
    }

    /// Construct a physical monitor. Drivers wrap their MMIO behind a
    /// custom monitor by passing the framebuffer they own. Currently
    /// monitors expose only the in-RAM software framebuffer; a real
    /// virtio-gpu driver would extend this.
    pub fn new_physical(name: &str, fb: Framebuffer) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            kind: MonitorKind::Physical,
            width: fb.width,
            height: fb.height,
            fb: Mutex::new(fb),
            id: AtomicU32::new(u32::MAX),
        })
    }

    pub(super) fn set_id(&self, id: MonitorId) {
        self.id.store(id.0, Ordering::Relaxed);
    }

    pub fn id(&self) -> MonitorId {
        MonitorId(self.id.load(Ordering::Relaxed))
    }

    /// Borrow the framebuffer for direct draw operations.
    pub fn with_framebuffer<R>(&self, f: impl FnOnce(&mut Framebuffer) -> R) -> R {
        let mut g = self.fb.lock();
        f(&mut g)
    }

    /// Copy the current framebuffer into a heap-allocated `Vec<u8>`.
    /// Used by virtual displays for snapshotting / remoting.
    pub fn snapshot(&self) -> alloc::vec::Vec<u8> {
        self.fb.lock().pixels().to_vec()
    }
}
