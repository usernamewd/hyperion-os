//! Syscall dispatch.
//!
//! User threads enter the kernel via `svc #0`, which lands in the
//! synchronous-from-EL0 (or EL1, today) handler in
//! [`crate::arch::aarch64::exceptions`]. That handler reads the syscall
//! number from `x8` and the first six arguments from `x0..x5`, then calls
//! [`dispatch`].
//!
//! This is the canonical place to add new syscalls. Numbers are stable
//! enums in [`SyscallNr`] so the [`hyperion_os_api`] crate can re-export
//! them and userland code can refer to them by name.

use core::convert::TryFrom;

pub use hyperion_os_api::syscall::SyscallNr;

/// Negative values returned by syscalls indicate `-errno`.
pub type SyscallResult = i64;

/// Dispatch table called from the synchronous trap handler.
pub fn dispatch(nr: u64, args: [u64; 6]) -> SyscallResult {
    match SyscallNr::try_from(nr).ok() {
        Some(SyscallNr::Yield) => sys_yield(),
        Some(SyscallNr::Exit) => sys_exit(args[0] as i32),
        Some(SyscallNr::PutChar) => sys_putchar(args[0] as u8),
        Some(SyscallNr::GetChar) => sys_getchar(),
        Some(SyscallNr::Uptime) => sys_uptime(),
        Some(SyscallNr::Reboot) => sys_reboot(),
        Some(SyscallNr::Shutdown) => sys_shutdown(),
        None => -1,
    }
}

fn sys_yield() -> SyscallResult {
    crate::proc::scheduler::yield_now();
    0
}

fn sys_exit(_code: i32) -> SyscallResult {
    crate::proc::scheduler::exit_current();
}

fn sys_putchar(b: u8) -> SyscallResult {
    crate::arch::aarch64::uart::putb(b);
    0
}

fn sys_getchar() -> SyscallResult {
    crate::arch::aarch64::uart::getb_blocking() as i64
}

fn sys_uptime() -> SyscallResult {
    let f = crate::arch::aarch64::timer::read_freq();
    let c = crate::arch::aarch64::timer::read_count();
    if f == 0 {
        0
    } else {
        ((c * 1000) / f) as i64
    }
}

fn sys_reboot() -> SyscallResult {
    crate::arch::aarch64::psci::system_reset();
}

fn sys_shutdown() -> SyscallResult {
    crate::arch::aarch64::psci::system_off();
}
