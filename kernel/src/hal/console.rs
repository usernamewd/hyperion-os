//! Active boot console.
//!
//! At early boot the boot stub instantiates one of the UART drivers
//! based on the [`super::BootInfo`] handed to it and stows it here. The
//! rest of the kernel — including the very first `log!` line — talks to
//! the active console through this single global.
//!
//! We deliberately don't use `Box<dyn Console>` because (a) the heap
//! isn't online yet when the first banner is printed and (b) the
//! kernel's TCB stays smaller without a heap dependency in the boot
//! console. Instead, we store concrete driver instances in an enum.

use crate::drivers::uart::{
    bcm_mini::BcmMiniUart, ns16550::Ns16550, pl011::Pl011, Console,
};
use crate::sync::Mutex;

use super::boot_info::ConsoleSpec;
use super::ConsoleKind;

/// Concrete driver instance picked at boot. Variants are kept directly
/// inline so we don't need the heap for the early console.
pub enum BootConsole {
    Pl011(Pl011),
    Ns16550(Ns16550),
    BcmMiniUart(BcmMiniUart),
}

impl Console for BootConsole {
    fn putb(&self, b: u8) {
        match self {
            BootConsole::Pl011(c) => c.putb(b),
            BootConsole::Ns16550(c) => c.putb(b),
            BootConsole::BcmMiniUart(c) => c.putb(b),
        }
    }

    fn getb(&self) -> Option<u8> {
        match self {
            BootConsole::Pl011(c) => c.getb(),
            BootConsole::Ns16550(c) => c.getb(),
            BootConsole::BcmMiniUart(c) => c.getb(),
        }
    }
}

static CONSOLE: Mutex<Option<BootConsole>> = Mutex::new(None);

/// Build and install the active console from [`ConsoleSpec`]. Safe to
/// call multiple times; later calls replace the active driver.
///
/// # Safety
/// `spec.regs.base` must be a valid, mapped MMIO base for the named
/// driver class.
pub unsafe fn install(spec: ConsoleSpec) {
    // SAFETY: caller asserts the MMIO base is mapped.
    let driver = unsafe {
        match spec.kind {
            ConsoleKind::Pl011 => {
                let d = Pl011::new(spec.regs.base as usize);
                d.init(spec.clock_hz);
                BootConsole::Pl011(d)
            }
            ConsoleKind::Ns16550 => {
                // Most modern 16550 instances use a 4-byte stride. We use
                // 1 byte if the register window is small (<0x80 bytes),
                // and 4 otherwise. Boards that need a different stride
                // can be handled in DTB-driven probe later.
                let stride = if spec.regs.size < 0x80 { 1 } else { 4 };
                let d = Ns16550::new(spec.regs.base as usize, stride);
                d.init(spec.clock_hz);
                BootConsole::Ns16550(d)
            }
            ConsoleKind::BcmMiniUart => {
                let d = BcmMiniUart::new(spec.regs.base as usize);
                d.init(spec.clock_hz);
                BootConsole::BcmMiniUart(d)
            }
        }
    };
    *CONSOLE.lock() = Some(driver);
}

/// Send a single byte through the active console. No-ops if the boot
/// stub never published one (which would be a bug).
pub fn putb(b: u8) {
    if let Some(c) = CONSOLE.lock().as_ref() {
        c.putb(b);
    }
}

/// Try to read a byte without blocking. `None` if the FIFO is empty *or*
/// if no console has been installed yet.
pub fn getb() -> Option<u8> {
    CONSOLE.lock().as_ref().and_then(|c| c.getb())
}

/// Block until a byte is received. Spins if no console has been
/// installed (we assume the boot stub will publish one momentarily).
pub fn getb_blocking() -> u8 {
    loop {
        if let Some(b) = getb() {
            return b;
        }
        core::hint::spin_loop();
    }
}

/// `core::fmt::Write` adapter that targets the active console.
pub struct Writer;

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if b == b'\n' {
                putb(b'\r');
            }
            putb(b);
        }
        Ok(())
    }
}
