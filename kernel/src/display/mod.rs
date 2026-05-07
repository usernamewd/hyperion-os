//! Display subsystem.
//!
//! Hyperion separates the *display surface* (a generic [`Framebuffer`]
//! pixel buffer) from the *monitor* (a physical screen, an HDMI port,
//! or a virtio-gpu virtual display). The [`compositor`] stacks
//! application [`crate::ui::Canvas`]es onto monitors.
//!
//! On boards where firmware hands us a framebuffer — UEFI GOP, a DTB
//! `simple-framebuffer` node, BCM mailbox, future virtio-gpu scanout
//! buffer — we register it as **monitor #0** so the compositor draws
//! straight to the physical screen. When no firmware framebuffer is
//! available we fall back to a 1280x720 in-RAM monitor (used for
//! headless boots and serial-only QEMU runs).
//!
//! Custom OSes can always register additional monitors at runtime
//! through [`register_monitor`].

pub mod compositor;
pub mod framebuffer;
pub mod monitor;

pub use compositor::Compositor;
pub use framebuffer::{Framebuffer, PixelFormat};
pub use monitor::{Monitor, MonitorId, MonitorKind};

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::hal::boot_info::PixelFormat as HalPixelFormat;
use crate::sync::Mutex;

static MONITORS: Mutex<Vec<Arc<Monitor>>> = Mutex::new(Vec::new());

/// One-time display init.
///
/// If the HAL discovered a firmware-provided framebuffer we register it
/// as monitor #0 (so the compositor draws to the physical screen
/// directly). Otherwise a 1280x720 heap-backed monitor is registered
/// as a fallback so a pure serial boot still has somewhere to render.
pub fn init() {
    let bi = crate::hal::info();
    if let Some(fb) = bi.framebuffer {
        let format = match fb.format {
            HalPixelFormat::Bgra8888 => PixelFormat::Bgra8,
            HalPixelFormat::Rgba8888 => PixelFormat::Rgba8,
        };
        // SAFETY: the HAL only publishes a `FramebufferInfo` after
        // verifying the region is mapped + sized; we are the sole
        // accessor (the compositor takes the monitor's lock).
        let scanout = unsafe {
            Framebuffer::from_mmio(
                fb.base as *mut u8,
                fb.width,
                fb.height,
                fb.stride_bytes,
                format,
            )
        };
        let m = Monitor::new_physical("fb0", scanout);
        register_monitor(m);
        crate::log::info!(
            "display: firmware framebuffer @ {:#x} {}x{} stride={} bpp={}",
            fb.base,
            fb.width,
            fb.height,
            fb.stride_bytes,
            fb.bpp
        );
    } else {
        let m = Monitor::new_virtual("monitor0", 1280, 720);
        register_monitor(m);
        crate::log::info!(
            "display: no firmware framebuffer; using virtual 1280x720 monitor"
        );
    }
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
