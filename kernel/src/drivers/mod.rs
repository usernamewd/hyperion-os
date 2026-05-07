//! Driver subsystem.
//!
//! Drivers are organised by class (UART, IRQ controller, virtio,
//! framebuffer, …) and decoupled from the kernel core via traits in
//! [`crate::hal`]. The HAL probes the discovered hardware (via DTB or
//! UEFI handoff) and instantiates the right concrete driver here.

pub mod uart;
