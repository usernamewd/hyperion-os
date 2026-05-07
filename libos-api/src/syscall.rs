//! Syscall ABI for Hyperion.
//!
//! User threads enter the kernel via `svc #0`. The conventions are:
//!
//! * Syscall number in `x8`.
//! * Up to 6 arguments in `x0..x5`.
//! * Return value in `x0`. Negative values are `-errno`.

/// Stable syscall numbers. Add new entries at the end; never re-use a slot.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallNr {
    Yield = 0,
    Exit = 1,
    PutChar = 2,
    GetChar = 3,
    Uptime = 4,
    Reboot = 5,
    Shutdown = 6,
}

impl core::convert::TryFrom<u64> for SyscallNr {
    type Error = ();
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        Ok(match v {
            0 => Self::Yield,
            1 => Self::Exit,
            2 => Self::PutChar,
            3 => Self::GetChar,
            4 => Self::Uptime,
            5 => Self::Reboot,
            6 => Self::Shutdown,
            _ => return Err(()),
        })
    }
}

/// Issue an `svc #0` syscall from userland. Linked-out of the kernel build
/// (because `svc` raises a synchronous exception there); userland binaries
/// targeting Hyperion would call this directly.
///
/// # Safety
/// The caller must obey the kernel ABI for the chosen syscall.
#[cfg(target_arch = "aarch64")]
#[inline]
pub unsafe fn raw_syscall(nr: SyscallNr, args: [u64; 6]) -> i64 {
    let ret: i64;
    // SAFETY: `svc #0` is the documented userland trap entrypoint.
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") nr as u64,
            inlateout("x0") args[0] => ret,
            in("x1") args[1],
            in("x2") args[2],
            in("x3") args[3],
            in("x4") args[4],
            in("x5") args[5],
            options(nostack)
        );
    }
    ret
}
