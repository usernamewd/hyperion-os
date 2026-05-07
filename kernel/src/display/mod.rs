//! Display subsystem.
//!
//! Hyperion separates the *display surface* (a generic [`Framebuffer`]
//! pixel buffer) from the *monitor* (a physical screen, an HDMI port, or a
//! virtio-gpu virtual display). The [`compositor`] stacks application
//! [`crate::ui::Canvas`]es onto monitors.
//!
//! On QEMU `virt`, real graphics arrive through `-device virtio-gpu-pci`;
//! a full virtio-gpu driver is out of scope for this milestone, so the
//! built-in [`monitor::Monitor`] backed by an in-RAM framebuffer is used
//! as the default. Custom OSes can register additional monitors at runtime
//! through [`register_monitor`].

pub mod compositor;
pub mod framebuffer;
pub mod monitor;

pub use compositor::Compositor;
pub use framebuffer::{Framebuffer, PixelFormat};
pub use monitor::{Monitor, MonitorId, MonitorKind};

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::Mutex;

static MONITORS: Mutex<Vec<Arc<Monitor>>> = Mutex::new(Vec::new());

/// One-time display init: register a default 1280x720 in-RAM monitor.
pub fn init() {
    let m = Monitor::new_virtual("monitor0", 1280, 720);
    register_monitor(m);
}

/// Register a new monitor with the display manager. Returns the ID
/// assigned to it (matches the order of registration).
pub fn register_monitor(m: Arc<Monitor>) -> MonitorId {
    let mut v = MONITORS.lock();
    let id = MonitorId(v.len() as u32);
    m.set_id(id);
    v.push(m);
    id
}

/// Get a monitor by ID.
pub fn get(id: MonitorId) -> Option<Arc<Monitor>> {
    MONITORS.lock().get(id.0 as usize).cloned()
}

/// Snapshot all registered monitors (for the shell `display` command).
pub fn list() -> Vec<Arc<Monitor>> {
    MONITORS.lock().clone()
}

/// Number of registered monitors.
pub fn count() -> usize {
    MONITORS.lock().len()
}
