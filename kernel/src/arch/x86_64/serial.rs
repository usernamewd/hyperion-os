//! Early COM1 (NS16550) serial init.
//!
//! Done before the HAL has a console driver installed so the
//! Multiboot2 trampoline can `outb` characters when something goes
//! wrong before [`crate::hal::init`] finishes.

use super::outb;

const COM1: u16 = 0x3F8;

/// Configure COM1 for 115200-8-N-1, no FIFO, no IRQs.
///
/// # Safety
/// Must run before the HAL installs its own console (which then takes
/// ownership of the same UART). Calling twice is harmless but wasteful.
pub unsafe fn early_init() {
    // SAFETY: legacy I/O port.
    unsafe {
        outb(COM1 + 1, 0x00); // disable interrupts
        outb(COM1 + 3, 0x80); // DLAB=1
        outb(COM1, 0x01); // divisor low (115200)
        outb(COM1 + 1, 0x00); // divisor high
        outb(COM1 + 3, 0x03); // DLAB=0, 8N1
        outb(COM1 + 2, 0xC7); // FIFO enable, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B); // RTS/DSR set, OUT2 raised
    }
}
