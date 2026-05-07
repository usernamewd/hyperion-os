//! Driver subsystem.
//!
//! Drivers are organised by class (UART, IRQ controller, virtio,
//! framebuffer, …) and decoupled from the kernel core via traits in
//! [`crate::hal`]. The HAL probes the discovered hardware (via DTB or
//! UEFI handoff) and instantiates the right concrete driver here.

pub mod block;
pub mod net;
pub mod uart;
pub mod virtio;

/// Late driver bring-up, called from [`crate::kmain`] after the
/// scheduler / IPC / FS / display subsystems are initialised.
///
/// This is where virtio devices are probed and wired into the rest of
/// the kernel: virtio-blk → block-device-backed FS, virtio-net →
/// smoltcp interface, virtio-gpu → display monitor.
pub fn init_late() {
    virtio::probe_all();
}
