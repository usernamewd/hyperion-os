//! EL0 (user mode) entry path.
//!
//! Hyperion is a microkernel: long term, almost everything outside the
//! tiny TCB will run at EL0 in its own address space. Today we have:
//!
//! 1. The kernel running at EL1 with caches/MMU off (identity-mapped).
//! 2. SVC #0 from EL0 dispatched through [`crate::syscall`] via the
//!    "Lower EL using aarch64 — synchronous" vector entry already
//!    installed by [`crate::arch::aarch64::exceptions`].
//! 3. A demo path in this module that drops a kernel thread to EL0,
//!    runs a tiny user program that issues SVCs (PutChar / Uptime /
//!    Exit), and gets back to the EL1 idle loop on exit.
//!
//! The full picture — multiple address spaces, ELF loading, EL0 page
//! tables, copy-{from,to}-user, signals — is bigger than this stub. But
//! the SVC dispatcher exists; this just demonstrates the round-trip and
//! gives the shell a smoke test for it.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

/// 16 KiB EL0 stack. Plenty for the demo program. Real user processes
/// will own their own stacks via the VMM.
const USER_STACK_SIZE: usize = 16 * 1024;

/// One-shot kernel thread that drops itself into EL0 and runs the demo
/// user payload.
extern "C" fn user_demo_thread(_arg: usize) -> ! {
    let mut stack: Vec<u8> = vec![0u8; USER_STACK_SIZE];
    let stack_top = stack.as_mut_ptr() as usize + USER_STACK_SIZE;
    // Round down to 16-byte alignment as required by AArch64.
    let stack_top = stack_top & !0xf;
    // Keep `stack` alive for the lifetime of the EL0 process. We leak
    // the Box; in a real implementation the thread struct would own it
    // and free it on exit_current().
    let stack: Box<[u8]> = stack.into_boxed_slice();
    let _ = Box::leak(stack);

    let entry = user_program as usize;
    crate::log::info!(
        "userdemo: dropping to EL0 (entry={:#x}, sp={:#x})",
        entry,
        stack_top
    );
    unsafe { enter_el0(entry, stack_top) };
}

/// Spawn a kernel thread that immediately drops to EL0 and exercises
/// the syscall dispatcher.
pub fn spawn_demo() -> crate::proc::Tid {
    crate::proc::scheduler::spawn("userdemo", user_demo_thread, 0)
}

/// Drop the current thread to EL0 with the given entry point and stack.
///
/// # Safety
/// `entry` must be a valid EL0-executable instruction stream (the demo
/// program is in `.text` which is identity-mapped and readable from
/// EL0 since SCTLR_EL1.UWXN=0). `sp` must be a 16-byte-aligned region
/// large enough to hold a small call stack and remain valid for the
/// lifetime of the EL0 process.
#[inline(never)]
pub unsafe fn enter_el0(entry: usize, sp: usize) -> ! {
    // SPSR_EL1 = EL0t (mode = 0b0000) with DAIF cleared so EL0 still
    // takes interrupts (timer ticks etc). DAIF clear is bits 9..6 = 0.
    let spsr: u64 = 0;
    // SAFETY: this is the standard "drop to EL0" sequence: load
    // ELR_EL1 / SPSR_EL1 / SP_EL0, then `eret`. The EL0 program runs
    // with the same physical mappings (MMU off / identity-mapped) so
    // it can access its own .text directly.
    unsafe {
        core::arch::asm!(
            "msr spsr_el1, {spsr}",
            "msr elr_el1,  {entry}",
            "msr sp_el0,   {sp}",
            "isb",
            "eret",
            spsr  = in(reg) spsr,
            entry = in(reg) entry as u64,
            sp    = in(reg) sp    as u64,
            options(noreturn, nostack)
        )
    }
}

/// Tiny EL0 program. Issues a few SVCs, exits.
///
/// Runs without a stdlib, no memory allocation, no calls into the
/// kernel beyond SVC #0.
#[no_mangle]
extern "C" fn user_program() -> ! {
    const NR_PUTCHAR: u64 = 2;
    const NR_UPTIME: u64 = 4;
    const NR_EXIT: u64 = 1;

    fn put(b: u8) {
        // SAFETY: SVC #0 is the kernel's syscall entry; nr=2 is PutChar
        // and reads x0 as the byte.
        unsafe {
            core::arch::asm!("svc #0",
                in("x8") NR_PUTCHAR,
                in("x0") b as u64,
                options(nomem, nostack, preserves_flags));
        }
    }
    let msg = b"\r\n[el0] hello from user mode!\r\n";
    for &b in msg {
        put(b);
    }
    // Read uptime via syscall.
    let ms: u64;
    // SAFETY: SVC #0 with nr=4 returns uptime in milliseconds.
    unsafe {
        core::arch::asm!("svc #0",
            in("x8") NR_UPTIME,
            lateout("x0") ms,
            options(nomem, nostack));
    }
    let mut buf = [b' '; 32];
    let mut idx = 0;
    let mut n = ms;
    if n == 0 {
        buf[idx] = b'0';
        idx += 1;
    } else {
        let mut tmp = [0u8; 32];
        let mut t = 0;
        while n > 0 {
            tmp[t] = b'0' + (n % 10) as u8;
            n /= 10;
            t += 1;
        }
        while t > 0 {
            t -= 1;
            buf[idx] = tmp[t];
            idx += 1;
        }
    }
    let prefix = b"[el0] uptime via syscall: ";
    for &b in prefix {
        put(b);
    }
    for &b in &buf[..idx] {
        put(b);
    }
    let suffix = b" ms\r\n";
    for &b in suffix {
        put(b);
    }

    // Exit cleanly. Returning here would cause the kernel to panic
    // because there's no return path on the user stack.
    // SAFETY: SVC #0 nr=1 with x0=status terminates the user thread.
    unsafe {
        core::arch::asm!("svc #0",
            in("x8") NR_EXIT,
            in("x0") 0u64,
            options(noreturn, nostack));
    }
}
