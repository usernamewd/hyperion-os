//! Power State Coordination Interface (PSCI) calls via HVC.
//!
//! QEMU exposes the standard PSCI 1.0 interface; we only need
//! `SYSTEM_OFF` and `SYSTEM_RESET` for the shell's `shutdown` / `reboot`
//! commands.

const PSCI_SYSTEM_OFF: u32 = 0x8400_0008;
const PSCI_SYSTEM_RESET: u32 = 0x8400_0009;

#[inline(never)]
pub fn system_off() -> ! {
    // SAFETY: HVC at EL1 is permitted under HCR_EL2.HCD=0 which is the QEMU
    // virt default; on real hardware with EL3 firmware this would be an
    // SMC instead.
    unsafe {
        core::arch::asm!("hvc #0", in("w0") PSCI_SYSTEM_OFF, options(nomem, nostack, noreturn));
    }
}

#[inline(never)]
pub fn system_reset() -> ! {
    // SAFETY: see [`system_off`].
    unsafe {
        core::arch::asm!("hvc #0", in("w0") PSCI_SYSTEM_RESET, options(nomem, nostack, noreturn));
    }
}
