//! 8250 / 16550-compatible UART (`ns16550a`, `snps,dw-apb-uart`,
//! `fsl,16550`, …).
//!
//! Used by NXP Layerscape, Marvell Armada, Allwinner H6+, Rockchip,
//! Tegra, riscv64-virt, and most generic AArch64 dev boards. Stride
//! between registers is fixed at 1 byte for the classic 16550 layout;
//! Synopsys / Tegra use a 32-bit stride. We pick the stride from the
//! `RegSpec.size` and a small heuristic, but if the board reports a
//! 0x100-aligned register block we treat it as 32-bit-stride which is
//! the modern convention.
//!
//! On x86_64 the same chip lives behind I/O ports (0x3F8 = COM1,
//! 0x2F8 = COM2, …). Set [`Ns16550::port_io`] to use `in/out`
//! instructions instead of MMIO; the rest of the register layout is
//! identical.

use super::Console;

pub struct Ns16550 {
    base: usize,
    stride: usize,
    /// When true, address `base..base+stride*N` as legacy x86 I/O
    /// ports (`in`/`out`) rather than memory-mapped registers. Always
    /// false on aarch64 — port I/O doesn't exist there.
    #[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
    port_io: bool,
}

const RHR: usize = 0; // RX (read)
const THR: usize = 0; // TX (write)
const IER: usize = 1; // interrupt enable
const FCR: usize = 2; // fifo control (write)
const LCR: usize = 3; // line control
const MCR: usize = 4; // modem control
const LSR: usize = 5; // line status
const DLL: usize = 0; // divisor low (DLAB=1)
const DLH: usize = 1; // divisor high (DLAB=1)

const LSR_THRE: u8 = 1 << 5;
const LSR_DR: u8 = 1 << 0;

impl Ns16550 {
    /// MMIO constructor.
    ///
    /// # Safety
    /// `base` must be the physical address of a 16550-compatible UART
    /// register block, identity-mapped for the kernel's lifetime.
    pub const unsafe fn new(base: usize, stride: usize) -> Self {
        Self {
            base,
            stride: if stride == 0 { 1 } else { stride },
            port_io: false,
        }
    }

    /// Port-I/O constructor for x86 platforms (COM1=0x3F8, COM2=0x2F8,
    /// …).
    ///
    /// # Safety
    /// `port` must be the base I/O port of a 16550-compatible UART. We
    /// program 8N1 / 115200 in [`Self::init`] before any caller pokes
    /// the device.
    #[cfg(target_arch = "x86_64")]
    pub const unsafe fn new_port(port: u16) -> Self {
        Self {
            base: port as usize,
            stride: 1,
            port_io: true,
        }
    }

    /// Initialise for 115200 8N1. `clock_hz` is the UART input clock; if
    /// zero, we skip baud-rate programming (assumes firmware already
    /// configured it, which is true on most U-Boot-style handoffs).
    ///
    /// # Safety
    /// Must be called once per device, before concurrent access.
    pub unsafe fn init(&self, clock_hz: u32) {
        // SAFETY: device-register writes; caller asserts exclusive access.
        unsafe {
            // Disable interrupts.
            self.w(IER, 0);
            if clock_hz != 0 {
                let baud = 115_200u32;
                let div = (clock_hz / (16 * baud)).max(1) as u16;
                // Set DLAB to access divisor latches.
                self.w(LCR, 0x80);
                self.w(DLL, (div & 0xff) as u8);
                self.w(DLH, ((div >> 8) & 0xff) as u8);
            }
            // 8N1, DLAB=0
            self.w(LCR, 0x03);
            // Enable + reset FIFOs, 14-byte trigger.
            self.w(FCR, 0xc7);
            // DTR + RTS + OUT2 (some chips need OUT2 for IRQs to leave the
            // chip even though we don't use them yet).
            self.w(MCR, 0x0b);
        }
    }

    #[inline(always)]
    unsafe fn w(&self, reg: usize, val: u8) {
        // SAFETY: caller asserts the device is mapped.
        #[cfg(target_arch = "x86_64")]
        if self.port_io {
            let port = (self.base + reg * self.stride) as u16;
            // SAFETY: legacy I/O port writes always succeed in ring 0.
            unsafe {
                crate::arch::x86_64::outb(port, val);
            }
            return;
        }
        unsafe {
            let p = (self.base + reg * self.stride) as *mut u8;
            core::ptr::write_volatile(p, val)
        }
    }

    #[inline(always)]
    unsafe fn r(&self, reg: usize) -> u8 {
        // SAFETY: caller asserts the device is mapped.
        #[cfg(target_arch = "x86_64")]
        if self.port_io {
            let port = (self.base + reg * self.stride) as u16;
            // SAFETY: legacy I/O port reads always succeed in ring 0.
            unsafe {
                return crate::arch::x86_64::inb(port);
            }
        }
        unsafe {
            let p = (self.base + reg * self.stride) as *const u8;
            core::ptr::read_volatile(p)
        }
    }
}

impl Console for Ns16550 {
    fn putb(&self, b: u8) {
        // SAFETY: device initialised before first call.
        unsafe {
            while self.r(LSR) & LSR_THRE == 0 {
                core::hint::spin_loop();
            }
            self.w(THR, b);
        }
    }

    fn getb(&self) -> Option<u8> {
        // SAFETY: device initialised before first call.
        unsafe {
            if self.r(LSR) & LSR_DR == 0 {
                None
            } else {
                Some(self.r(RHR))
            }
        }
    }
}
