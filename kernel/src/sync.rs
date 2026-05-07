//! Kernel synchronisation primitives.
//!
//! Re-exports of [`spin`] types under shorter names so the rest of the
//! kernel doesn't have to know which crate they come from. We also
//! provide an `IrqLock` guard wrapper for critical sections that must
//! be uninterruptible (the scheduler's runqueue, mainly).

pub use spin::{Mutex, MutexGuard, Once, RwLock};

/// RAII guard that masks IRQs while held.
pub struct IrqLock {
    saved: u64,
}

impl IrqLock {
    /// Mask IRQs and remember the previous interrupt-mask state.
    pub fn new() -> Self {
        let saved = crate::arch::read_irq_mask();
        crate::arch::disable_irqs();
        Self { saved }
    }
}

impl Default for IrqLock {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IrqLock {
    fn drop(&mut self) {
        // SAFETY: `saved` was produced by `read_irq_mask` above.
        unsafe {
            crate::arch::write_irq_mask(self.saved);
        }
    }
}
