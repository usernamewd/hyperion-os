//! Architecture-specific entry points and facade.
//!
//! Hyperion targets aarch64 and amd64/x86_64. Each backend lives in its
//! own submodule (`arch::aarch64`, `arch::x86_64`) and implements a
//! small handful of functions that the rest of the kernel calls through
//! this facade — the goal being that nothing outside `arch::*` and the
//! `panic_handler` ever has to write `cfg(target_arch = ...)`.
//!
//! Adding a new architecture amounts to:
//!
//! 1. Implementing `boot`, `late_init`, `halt`, `disable_irqs`,
//!    `enable_irqs`, plus a `timer` module exporting `read_freq` and
//!    `read_count`.
//! 2. Providing a linker script and a `BootInfo` builder for the
//!    relevant boot protocol.
//! 3. Wiring the panic-handler-side halt into [`halt`].

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

/// Late architecture initialisation, called from [`crate::kmain`] after
/// the memory subsystem is online.
pub fn late_init() {
    #[cfg(target_arch = "aarch64")]
    aarch64::late_init();
    #[cfg(target_arch = "x86_64")]
    x86_64::late_init();
}

/// Bring up secondary CPUs, if the architecture supports SMP.
///
/// On aarch64 this issues PSCI CPU_ON for every secondary up to
/// [`aarch64::boot::MAX_CPUS`]; secondaries land in
/// [`aarch64::smp::hyperion_kernel_secondary_main`] and online
/// themselves. On x86_64 we currently boot only the BSP, so this is a
/// no-op.
pub fn start_secondaries() {
    #[cfg(target_arch = "aarch64")]
    aarch64::smp::start_secondaries();
}

/// Halt the current CPU forever. Disables interrupts before stalling so
/// nothing can wake us back up. Used as the final stop in panic /
/// shutdown paths.
pub fn halt() -> ! {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::halt()
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::halt()
    }
}

/// Mask architecture interrupts (DAIF / RFLAGS.IF). Used by panic and
/// scheduler critical sections.
pub fn disable_irqs() {
    #[cfg(target_arch = "aarch64")]
    aarch64::exceptions::disable_irqs();
    #[cfg(target_arch = "x86_64")]
    x86_64::exceptions::disable_irqs();
}

/// Unmask architecture interrupts.
pub fn enable_irqs() {
    #[cfg(target_arch = "aarch64")]
    aarch64::exceptions::enable_irqs();
    #[cfg(target_arch = "x86_64")]
    x86_64::exceptions::enable_irqs();
}

/// Snapshot the current interrupt-mask register so [`crate::sync::IrqLock`]
/// can restore it on drop.
#[inline]
pub fn read_irq_mask() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        let v: u64;
        // SAFETY: reading DAIF at EL1 is privileged but always safe.
        unsafe {
            core::arch::asm!("mrs {0}, daif", out(reg) v, options(nomem, nostack));
        }
        v
    }
    #[cfg(target_arch = "x86_64")]
    {
        let v: u64;
        // SAFETY: pushfq/pop is always safe.
        unsafe {
            core::arch::asm!("pushfq; pop {0}", out(reg) v, options(nomem));
        }
        v
    }
}

/// Restore an interrupt-mask register snapshot from [`read_irq_mask`].
///
/// # Safety
/// `mask` must come from a prior call to [`read_irq_mask`]; otherwise the
/// CPU may end up running with an unexpected interrupt state.
#[inline]
pub unsafe fn write_irq_mask(mask: u64) {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: writing DAIF at EL1 is privileged but always safe.
    unsafe {
        core::arch::asm!("msr daif, {0}", in(reg) mask, options(nomem, nostack));
    }
    #[cfg(target_arch = "x86_64")]
    // SAFETY: push/popfq restoring the full RFLAGS image.
    unsafe {
        core::arch::asm!("push {0}; popfq", in(reg) mask, options(nomem));
    }
}

/// Power off the system gracefully. On aarch64 this is a PSCI HVC; on
/// x86_64 it goes through QEMU's `0x604` ACPI shutdown port (and falls
/// back to halting on real hardware where that won't work).
pub fn system_off() -> ! {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::psci::system_off()
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::acpi::system_off()
    }
}

/// Reset the system. On aarch64 this is a PSCI HVC; on x86_64 it
/// triple-faults via the keyboard controller.
pub fn system_reset() -> ! {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::psci::system_reset()
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::acpi::system_reset()
    }
}

/// Read the boot CPU's monotonic counter frequency in Hz.
pub fn timer_freq() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::timer::read_freq()
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::timer::read_freq()
    }
}

/// Read the boot CPU's monotonic counter value (units of [`timer_freq`]).
pub fn timer_count() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::timer::read_count()
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::timer::read_count()
    }
}

/// Architecture-native idle instruction: stall this CPU until the next
/// interrupt wakes us. WFI on aarch64, HLT on x86_64.
#[inline]
pub fn idle() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: WFI is unprivileged at EL0 + supported at EL1; it just
    // stalls until an interrupt becomes pending.
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack));
    }
    #[cfg(target_arch = "x86_64")]
    // SAFETY: HLT is privileged but always safe in ring 0.
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
}
