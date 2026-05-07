//! Broadcom BCM2835 / BCM2837 mini-UART (Raspberry Pi 3+).
//!
//! When booting a Pi without UEFI firmware, the GPU bootloader chooses
//! the mini-UART (`AUX_MU_*`) as the default console if `enable_uart=1`
//! is set in `config.txt`. The mini-UART is a stripped-down 16550-ish
//! device with only a subset of registers.
//!
//! This driver is only used when the DTB advertises
//! `brcm,bcm2835-aux-uart`; on a UEFI-booted Pi 4 the firmware exposes a
//! standard PL011 instead and we use [`super::pl011`].

use super::Console;

pub struct BcmMiniUart {
    base: usize,
}

const AUX_MU_IO: usize = 0x40;
const AUX_MU_IER: usize = 0x44;
const AUX_MU_IIR: usize = 0x48;
const AUX_MU_LCR: usize = 0x4c;
const AUX_MU_MCR: usize = 0x50;
const AUX_MU_LSR: usize = 0x54;
const AUX_MU_CNTL: usize = 0x60;
const AUX_MU_BAUD: usize = 0x68;
// AUX_ENABLES is at AUX_BASE + 0x4; its base is AUX_MU_BASE - 0x40.

const LSR_DR: u32 = 1 << 0;
const LSR_THRE: u32 = 1 << 5;

impl BcmMiniUart {
    /// `base` is the AUX_MU_BASE (the *mini-UART* register window),
    /// typically 0x3F21_5040 on Pi 3 / 0xFE21_5040 on Pi 4.
    ///
    /// # Safety
    /// `base` must point to the BCM aux-UART register block, mapped.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    /// Minimal setup: 8N1, no interrupts. We assume the firmware has
    /// already set the GPIO alt-functions for TX/RX and enabled AUX_MU
    /// in `AUX_ENABLES` (which it does when `enable_uart=1`). Programming
    /// the baud divisor needs the core/VPU clock which is board-revision
    /// dependent; if `clock_hz` is non-zero we use it, otherwise we leave
    /// the divisor alone.
    ///
    /// # Safety
    /// Must be called once per device, before concurrent access.
    pub unsafe fn init(&self, clock_hz: u32) {
        // SAFETY: device-register writes; caller asserts exclusive access.
        unsafe {
            self.w(AUX_MU_CNTL, 0); // disable RX/TX while configuring
            self.w(AUX_MU_IER, 0); // mask all interrupts
            self.w(AUX_MU_IIR, 0xc6); // clear FIFOs
            self.w(AUX_MU_LCR, 0x03); // 8-bit (note: erratum, set 0x03 for 8 bits)
            self.w(AUX_MU_MCR, 0x00);
            if clock_hz != 0 {
                let baud = 115_200u32;
                // mini-UART baud_reg = (clock_hz / (8 * baud)) - 1
                let div = (clock_hz / (8 * baud)).saturating_sub(1);
                self.w(AUX_MU_BAUD, div);
            }
            self.w(AUX_MU_CNTL, 0x03); // enable RX + TX
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

impl Console for BcmMiniUart {
    fn putb(&self, b: u8) {
        // SAFETY: device initialised before first call.
        unsafe {
            while self.r(AUX_MU_LSR) & LSR_THRE == 0 {
                core::hint::spin_loop();
            }
            self.w(AUX_MU_IO, b as u32);
        }
    }

    fn getb(&self) -> Option<u8> {
        // SAFETY: device initialised before first call.
        unsafe {
            if self.r(AUX_MU_LSR) & LSR_DR == 0 {
                None
            } else {
                Some((self.r(AUX_MU_IO) & 0xff) as u8)
            }
        }
    }
}
