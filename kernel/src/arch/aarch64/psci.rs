//! Power State Coordination Interface (PSCI) calls via HVC.
//!
//! QEMU exposes the standard PSCI 1.0 interface; we use:
//!
//! * `SYSTEM_OFF` / `SYSTEM_RESET` for the shell's `shutdown` / `reboot`.
//! * `CPU_ON` to wake secondary CPUs out of their reset park loop and
//!   point them at our `_start_secondary` entry.
//!
//! On QEMU virt the conduit is HVC; on platforms that have an EL3
//! firmware (Trusted Firmware-A) the same calls are routed via SMC.

const PSCI_VERSION: u32 = 0x8400_0000;
const PSCI_CPU_ON: u32 = 0xc400_0003;
const PSCI_SYSTEM_OFF: u32 = 0x8400_0008;
const PSCI_SYSTEM_RESET: u32 = 0x8400_0009;

// PSCI return codes (a subset; see ARM DEN 0022).
pub const PSCI_SUCCESS: i32 = 0;
pub const PSCI_NOT_SUPPORTED: i32 = -1;
pub const PSCI_INVALID_PARAMETERS: i32 = -2;
pub const PSCI_DENIED: i32 = -3;
pub const PSCI_ALREADY_ON: i32 = -4;

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

/// Read the PSCI implementation version. The high 16 bits are the major
/// version, the low 16 bits the minor.
pub fn version() -> u32 {
    let v: u64;
    // SAFETY: PSCI_VERSION is informational and never modifies state.
    unsafe {
        core::arch::asm!(
            "hvc #0",
            in("w0") PSCI_VERSION,
            lateout("x0") v,
            options(nostack, preserves_flags)
        );
    }
    v as u32
}

/// Bring a secondary CPU online.
///
/// `target_mpidr` is the MPIDR_EL1 affinity value of the CPU to wake
/// (e.g. `0x1` for CPU 1 on QEMU virt's flat cluster). `entry_point` is
/// the physical address that CPU should start executing at, in EL1, with
/// caches/MMU off and `x0` set to `context_id`.
///
/// Returns 0 on success or a negative PSCI error code on failure.
pub fn cpu_on(target_mpidr: u64, entry_point: u64, context_id: u64) -> i32 {
    let ret: u64;
    // SAFETY: HVC #0 with PSCI_CPU_ON; the secondary CPU will start
    // executing at `entry_point` in EL1 with `x0` = `context_id`. The
    // caller must have published whatever data structures the new CPU
    // is going to need before this call.
    unsafe {
        core::arch::asm!(
            "dsb sy",
            "hvc #0",
            in("w0") PSCI_CPU_ON,
            in("x1") target_mpidr,
            in("x2") entry_point,
            in("x3") context_id,
            lateout("x0") ret,
            options(nostack)
        );
    }
    ret as i32
}
