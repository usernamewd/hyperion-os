//! ARM Generic Timer (CNTPCT / CNTP_TVAL) driver.
//!
//! Used by the scheduler as a periodic preemption tick. The PPI for the
//! EL1 physical timer is interrupt 30 in the GIC.

const TIMER_PPI: u32 = 30;

/// Initialise the EL1 physical timer at ~100 Hz and enable its interrupt.
pub fn init() {
    let freq = read_freq();
    let interval = freq / 100; // 10 ms
                               // SAFETY: privileged register writes are always safe at EL1.
    unsafe {
        core::arch::asm!("msr cntp_tval_el0, {0}", in(reg) interval);
        // CNTP_CTL_EL0: ENABLE=1, IMASK=0
        core::arch::asm!("msr cntp_ctl_el0, {0}", in(reg) 1u64);
    }
    super::gic::enable_ppi(TIMER_PPI);
}

/// Read the timer frequency in Hz.
pub fn read_freq() -> u64 {
    let v: u64;
    // SAFETY: reading CNTFRQ_EL0 is unprivileged.
    unsafe {
        core::arch::asm!("mrs {0}, cntfrq_el0", out(reg) v);
    }
    v
}

/// Read the current physical counter value.
pub fn read_count() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("isb; mrs {0}, cntpct_el0", out(reg) v);
    }
    v
}

/// Re-arm the timer for the next tick (10 ms).
pub fn rearm() {
    let freq = read_freq();
    let interval = freq / 100;
    // SAFETY: privileged register write.
    unsafe {
        core::arch::asm!("msr cntp_tval_el0, {0}", in(reg) interval);
    }
}

/// Returns true if the IRQ ID corresponds to the EL1 physical timer.
pub fn is_timer_irq(intid: u32) -> bool {
    intid == TIMER_PPI
}
