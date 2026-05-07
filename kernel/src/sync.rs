//! Kernel synchronisation primitives.
//!
//! Re-exports of [`spin`] types under shorter names so the rest of the
//! kernel doesn't have to know which crate they come from. We also gate
//! everything on a `WithIrqsOff` wrapper for cases where a critical
//! section must be uninterruptible (the scheduler's runqueue, mainly).

pub use spin::{Mutex, MutexGuard, Once, RwLock};

use crate::arch::aarch64::exceptions;

/// RAII guard that masks IRQs while held.
pub struct IrqLock {
    daif: u64,
}

impl IrqLock {
    /// Mask IRQs and remember the previous DAIF value.
    pub fn new() -> Self {
        let daif: u64;
        // SAFETY: reading and modifying DAIF at EL1 is privileged but
        // always safe.
        unsafe {
            core::arch::asm!("mrs {0}, daif", out(reg) daif, options(nomem, nostack));
        }
        exceptions::disable_irqs();
        Self { daif }
    }
}

impl Drop for IrqLock {
    fn drop(&mut self) {
        // SAFETY: restores previous DAIF.
        unsafe {
            core::arch::asm!("msr daif, {0}", in(reg) self.daif, options(nomem, nostack));
        }
    }
}
