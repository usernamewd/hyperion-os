//! UART drivers + the `Console` trait that abstracts them.
//!
//! Drivers in this module never call into the rest of the kernel; they're
//! pure register-poke code. The HAL holds whichever instance the boot
//! stub picked, behind a `&'static dyn Console`. Higher layers
//! (`crate::log`, `crate::shell`) talk only to that trait object.

use core::fmt;

pub mod bcm_mini;
pub mod ns16550;
pub mod pl011;

/// Minimum UART operations the kernel core relies on. Implementations must
/// be safe to call from any CPU once initialised; serialisation is handled
/// at the [`crate::log`] layer with a spin mutex around the trait object.
pub trait Console: Send + Sync {
    /// Send a single byte. Blocks while the TX FIFO is full.
    fn putb(&self, b: u8);

    /// Try to read a single byte without blocking. `None` if the RX FIFO
    /// is empty.
    fn getb(&self) -> Option<u8>;

    /// Block until a byte is received. Default: spin on [`Self::getb`].
    fn getb_blocking(&self) -> u8 {
        loop {
            if let Some(b) = self.getb() {
                return b;
            }
            core::hint::spin_loop();
        }
    }
}

/// Adapter so any [`Console`] can be used with `core::fmt::Write`.
pub struct ConsoleWriter<'a>(pub &'a dyn Console);

impl fmt::Write for ConsoleWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            // Convert lone `\n` to `\r\n` so QEMU stdio + real serial
            // terminals both render correctly.
            if b == b'\n' {
                self.0.putb(b'\r');
            }
            self.0.putb(b);
        }
        Ok(())
    }
}
