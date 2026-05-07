//! PL011 UART driver for QEMU `virt`.
//!
//! The PL011 is at physical 0x0900_0000 on the QEMU virt machine. We map it
//! identity (the kernel runs with paging off until [`super::mmu`] enables
//! it later) and treat it as a tiny memory-mapped FIFO.
//!
//! All UART access goes through [`Uart`], which is `Send + Sync` because we
//! only do single-byte writes; the underlying register interface is
//! atomic-safe at this granularity. Higher layers serialise writes via a
//! [`spin::Mutex`] in [`crate::log`].

use core::fmt::{self, Write};

const UART_BASE: usize = 0x0900_0000;

const UARTDR: usize = 0x000;
const UARTFR: usize = 0x018;
const UARTIBRD: usize = 0x024;
const UARTFBRD: usize = 0x028;
const UARTLCR_H: usize = 0x02c;
const UARTCR: usize = 0x030;
const UARTIMSC: usize = 0x038;
const UARTICR: usize = 0x044;

const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;

#[inline(always)]
unsafe fn write_reg(off: usize, val: u32) {
    // SAFETY: caller asserts the UART MMIO is mapped (true while the MMU is
    // off, and identity-mapped after we enable paging).
    unsafe { core::ptr::write_volatile((UART_BASE + off) as *mut u32, val) }
}

#[inline(always)]
unsafe fn read_reg(off: usize) -> u32 {
    // SAFETY: see `write_reg`.
    unsafe { core::ptr::read_volatile((UART_BASE + off) as *const u32) }
}

/// Initialise the PL011 at 115200 8N1. Must be called once at boot before
/// any other UART access.
pub unsafe fn init() {
    // Disable UART while we configure it.
    unsafe {
        write_reg(UARTCR, 0);
        // Clear pending interrupts.
        write_reg(UARTICR, 0x7ff);
        // Mask all interrupts.
        write_reg(UARTIMSC, 0);
        // QEMU's UART clock is 24 MHz. For 115200 baud:
        //   div = 24_000_000 / (16 * 115200) = 13.0208333...
        //   IBRD = 13, FBRD = round(0.0208 * 64) = 1
        write_reg(UARTIBRD, 13);
        write_reg(UARTFBRD, 1);
        // 8 bits, FIFO enabled, no parity.
        write_reg(UARTLCR_H, (0b11 << 5) | (1 << 4));
        // Enable: UART + TX + RX.
        write_reg(UARTCR, (1 << 0) | (1 << 8) | (1 << 9));
    }
}

/// Send a single byte, blocking while the TX FIFO is full.
pub fn putb(b: u8) {
    // SAFETY: UART has been initialised.
    unsafe {
        while read_reg(UARTFR) & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        write_reg(UARTDR, b as u32);
    }
}

/// Try to read a single byte if one is available. Returns `None` otherwise;
/// never blocks.
pub fn getb() -> Option<u8> {
    // SAFETY: UART has been initialised.
    unsafe {
        if read_reg(UARTFR) & FR_RXFE != 0 {
            None
        } else {
            Some((read_reg(UARTDR) & 0xff) as u8)
        }
    }
}

/// Block until a byte is received.
pub fn getb_blocking() -> u8 {
    loop {
        if let Some(b) = getb() {
            return b;
        }
        core::hint::spin_loop();
    }
}

/// Convenience writer for `core::fmt`.
pub struct Uart;

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            putb(b);
        }
        Ok(())
    }
}
