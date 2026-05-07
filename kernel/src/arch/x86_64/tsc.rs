//! Time Stamp Counter — wall-clock tick source.
//!
//! On every CPU since the Pentium, `RDTSC` is a free-running
//! 64-bit cycle counter. On modern CPUs it ticks at the *invariant*
//! TSC frequency (independent of the core's actual frequency), which
//! makes it a perfect cheap monotonic clock.
//!
//! [`crate::arch::timer_count`] / [`crate::arch::timer_freq`] expose
//! it to the rest of the kernel through the architecture facade.

use core::sync::atomic::{AtomicU64, Ordering};

static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(0);

/// Read the TSC. Wraps `rdtsc`.
#[inline]
pub fn read() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdtsc is unprivileged; CR4.TSD=0 by default.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | lo as u64
}

/// Cache the calibrated TSC frequency in Hz. Read by [`read_freq`].
pub fn set_freq(hz: u64) {
    TSC_FREQ_HZ.store(hz, Ordering::Release);
}

/// Read the TSC frequency in Hz, defaulting to 1 GHz if calibration
/// hasn't run (which would only happen if the timer subsystem hasn't
/// been initialised yet — better to overestimate than divide-by-zero).
pub fn read_freq() -> u64 {
    let v = TSC_FREQ_HZ.load(Ordering::Acquire);
    if v == 0 {
        1_000_000_000
    } else {
        v
    }
}
