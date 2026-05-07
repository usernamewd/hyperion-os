//! ARM PL011 PrimeCell UART (`arm,pl011`, `arm,primecell`).
//!
//! Used by:
//!
//! * QEMU `virt` machine (default console at `0x0900_0000`).
//! * Most ARM dev / server boards' debug consoles.
//! * Apple-AArch64 (M1/M2) Asahi-style UART is *not* PL011, so this
//!   driver is not used there.
//!
//! Initialisation programs the baud-rate generator for 115200 8N1
//! assuming the input clock advertised by the boot stub (24 MHz on
//! QEMU virt; pass `clock_hz` from the DTB on real hardware).

use super::Console;

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

pub struct Pl011 {
    base: usize,
}

impl Pl011 {
    /// # Safety
    /// `base` must be the physical address of a PL011 register block,
    /// identity-mapped readable+writable for the lifetime of the kernel.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    /// Configure for 115200 8N1 given an input clock in Hz. Call once at
    /// boot. `clock_hz` of 0 falls back to QEMU virt's 24 MHz.
    ///
    /// # Safety
    /// Must be called only once per device, before any concurrent access.
    pub unsafe fn init(&self, clock_hz: u32) {
        let clk = if clock_hz == 0 { 24_000_000 } else { clock_hz };
        let baud = 115_200u32;
        // PL011 UARTBAUDDIV = clk / (16 * baud); split into integer +
        // 6-bit fractional parts.
        let div_x64 = ((clk as u64) * 4) / (baud as u64); // = 64 * (clk/(16*baud))
        let ibrd = (div_x64 >> 6) as u32;
        let fbrd = (div_x64 & 0x3f) as u32;
        // SAFETY: register-block writes; caller asserts exclusive access.
        unsafe {
            self.w(UARTCR, 0);
            self.w(UARTICR, 0x7ff);
            self.w(UARTIMSC, 0);
            self.w(UARTIBRD, ibrd);
            self.w(UARTFBRD, fbrd);
            // 8 bits, FIFO enabled, no parity.
            self.w(UARTLCR_H, (0b11 << 5) | (1 << 4));
            // UART + TX + RX enabled.
            self.w(UARTCR, (1 << 0) | (1 << 8) | (1 << 9));
        }
    }

    #[inline(always)]
    unsafe fn w(&self, off: usize, val: u32) {
        // SAFETY: caller asserts the device is mapped.
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u32, val) }
    }
    #[inline(always)]
    unsafe fn r(&self, off: usize) -> u32 {
        // SAFETY: caller asserts the device is mapped.
        unsafe { core::ptr::read_volatile((self.base + off) as *const u32) }
    }
}

impl Console for Pl011 {
    fn putb(&self, b: u8) {
        // SAFETY: device initialised before first call.
        unsafe {
            while self.r(UARTFR) & FR_TXFF != 0 {
                core::hint::spin_loop();
            }
            self.w(UARTDR, b as u32);
        }
    }

    fn getb(&self) -> Option<u8> {
        // SAFETY: device initialised before first call.
        unsafe {
            if self.r(UARTFR) & FR_RXFE != 0 {
                None
            } else {
                Some((self.r(UARTDR) & 0xff) as u8)
            }
        }
    }
}
