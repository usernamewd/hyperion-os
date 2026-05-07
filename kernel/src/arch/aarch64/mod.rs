//! aarch64 architecture support.
//!
//! Hyperion is designed for ARMv8-A executing at EL1 (the conventional
//! kernel privilege level when launched by a hypervisor or by the
//! aarch64 Linux boot protocol).
//!
//! The boot path is:
//!
//! 1. QEMU drops us at `_start` in EL1 (or higher, in which case we drop
//!    down to EL1).
//! 2. We park secondary CPUs in a WFE loop, set up a stack, zero `.bss`,
//!    install a tiny exception vector that just halts on entry, and call
//!    [`crate::kmain`].
//! 3. From `kmain`, [`late_init`] is called once memory and the heap are
//!    ready. It installs the real exception vector, programs the generic
//!    timer, and brings up the GIC.

pub mod boot;
pub mod exceptions;
pub mod gic;
pub mod mmu;
pub mod psci;
pub mod timer;
pub mod uart;

/// Late architecture initialisation, called from [`crate::kmain`] after the
/// memory subsystem is ready.
pub fn late_init() {
    exceptions::install_vectors();
    gic::init();
    timer::init();
}

/// Halt the current CPU forever. Used as the final stop in panic / shutdown
/// paths. Disables interrupts first so we cannot be preempted out of the
/// halt loop.
pub fn halt() -> ! {
    // SAFETY: writing DAIF is privileged but always safe.
    unsafe {
        core::arch::asm!("msr daifset, #0xf", options(nomem, nostack));
    }
    loop {
        // SAFETY: WFE is always safe; it just stalls the core.
        unsafe {
            core::arch::asm!("wfe", options(nomem, nostack));
        }
    }
}

/// Read the current Exception Level (0..=3) from `CurrentEL[3:2]`.
#[inline]
pub fn current_el() -> u8 {
    let v: u64;
    // SAFETY: reading CurrentEL is unprivileged at every EL we run at.
    unsafe {
        core::arch::asm!("mrs {0}, CurrentEL", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    ((v >> 2) & 0b11) as u8
}
