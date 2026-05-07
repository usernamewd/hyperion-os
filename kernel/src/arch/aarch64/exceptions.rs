//! aarch64 exception vector table.
//!
//! ARMv8-A defines 16 exception entry points, organised as four groups of
//! four. Each entry is exactly 0x80 bytes wide and the table itself must
//! be 0x800-aligned. We provide a uniform stub for every entry that builds
//! a [`TrapFrame`] and dispatches to a Rust handler.
//!
//! For now most exceptions just log and halt; the synchronous-from-EL1
//! handler is wired up to [`crate::syscall`] so SVC instructions become
//! kernel syscalls.

use core::arch::global_asm;

/// CPU register state captured on exception entry.
#[repr(C)]
#[derive(Debug)]
pub struct TrapFrame {
    pub regs: [u64; 31], // x0..x30
    pub sp: u64,
    pub elr: u64,
    pub spsr: u64,
    pub esr: u64,
    pub far: u64,
}

global_asm!(
    r#"
    .macro SAVE_REGS
        sub     sp, sp, #(32 * 8 + 4 * 8)
        stp     x0,  x1,  [sp, #(0 * 16)]
        stp     x2,  x3,  [sp, #(1 * 16)]
        stp     x4,  x5,  [sp, #(2 * 16)]
        stp     x6,  x7,  [sp, #(3 * 16)]
        stp     x8,  x9,  [sp, #(4 * 16)]
        stp     x10, x11, [sp, #(5 * 16)]
        stp     x12, x13, [sp, #(6 * 16)]
        stp     x14, x15, [sp, #(7 * 16)]
        stp     x16, x17, [sp, #(8 * 16)]
        stp     x18, x19, [sp, #(9 * 16)]
        stp     x20, x21, [sp, #(10 * 16)]
        stp     x22, x23, [sp, #(11 * 16)]
        stp     x24, x25, [sp, #(12 * 16)]
        stp     x26, x27, [sp, #(13 * 16)]
        stp     x28, x29, [sp, #(14 * 16)]
        // x30 (LR), saved with sp at next slot
        mrs     x9,  sp_el0
        stp     x30, x9,  [sp, #(15 * 16)]
        mrs     x10, elr_el1
        mrs     x11, spsr_el1
        stp     x10, x11, [sp, #(16 * 16)]
        mrs     x10, esr_el1
        mrs     x11, far_el1
        stp     x10, x11, [sp, #(17 * 16)]
    .endm

    .macro RESTORE_REGS
        ldp     x10, x11, [sp, #(16 * 16)]
        msr     elr_el1,  x10
        msr     spsr_el1, x11
        ldp     x30, x9,  [sp, #(15 * 16)]
        msr     sp_el0,   x9
        ldp     x28, x29, [sp, #(14 * 16)]
        ldp     x26, x27, [sp, #(13 * 16)]
        ldp     x24, x25, [sp, #(12 * 16)]
        ldp     x22, x23, [sp, #(11 * 16)]
        ldp     x20, x21, [sp, #(10 * 16)]
        ldp     x18, x19, [sp, #(9 * 16)]
        ldp     x16, x17, [sp, #(8 * 16)]
        ldp     x14, x15, [sp, #(7 * 16)]
        ldp     x12, x13, [sp, #(6 * 16)]
        ldp     x10, x11, [sp, #(5 * 16)]
        ldp     x8,  x9,  [sp, #(4 * 16)]
        ldp     x6,  x7,  [sp, #(3 * 16)]
        ldp     x4,  x5,  [sp, #(2 * 16)]
        ldp     x2,  x3,  [sp, #(1 * 16)]
        ldp     x0,  x1,  [sp, #(0 * 16)]
        add     sp, sp, #(32 * 8 + 4 * 8)
    .endm

    .section .text
    .balign 0x800
    .globl __vectors
__vectors:
    // Current EL with SP_EL0 (we never use this — kernel runs SPSel=1)
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled

    // Current EL with SP_ELx (SPSel=1) — kernel-mode exceptions
    .balign 0x80
    b       __vec_sync_kernel
    .balign 0x80
    b       __vec_irq_kernel
    .balign 0x80
    b       __vec_fiq_kernel
    .balign 0x80
    b       __vec_serror_kernel

    // Lower EL using aarch64 — userspace
    .balign 0x80
    b       __vec_sync_user
    .balign 0x80
    b       __vec_irq_user
    .balign 0x80
    b       __vec_fiq_user
    .balign 0x80
    b       __vec_serror_user

    // Lower EL using aarch32 — unsupported
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled
    .balign 0x80
    b       __vec_unhandled

__vec_unhandled:
    SAVE_REGS
    mov     x0, sp
    mov     x1, #0
    bl      hyperion_trap_unhandled
    RESTORE_REGS
    eret

__vec_sync_kernel:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_sync
    RESTORE_REGS
    eret

__vec_irq_kernel:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_irq
    RESTORE_REGS
    eret

__vec_fiq_kernel:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_irq
    RESTORE_REGS
    eret

__vec_serror_kernel:
    SAVE_REGS
    mov     x0, sp
    mov     x1, #1
    bl      hyperion_trap_unhandled
    RESTORE_REGS
    eret

__vec_sync_user:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_sync
    RESTORE_REGS
    eret

__vec_irq_user:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_irq
    RESTORE_REGS
    eret

__vec_fiq_user:
    SAVE_REGS
    mov     x0, sp
    bl      hyperion_trap_irq
    RESTORE_REGS
    eret

__vec_serror_user:
    SAVE_REGS
    mov     x0, sp
    mov     x1, #2
    bl      hyperion_trap_unhandled
    RESTORE_REGS
    eret
"#
);

/// Install the real exception vector table into `VBAR_EL1` and unmask
/// IRQs/FIQs in `DAIF`.
pub fn install_vectors() {
    extern "C" {
        static __vectors: u8;
    }
    // SAFETY: writing VBAR_EL1 is privileged but always safe at EL1; the
    // address is the start of our 0x800-aligned vector table.
    unsafe {
        let addr = (&__vectors as *const u8) as u64;
        core::arch::asm!(
            "msr vbar_el1, {0}; isb",
            in(reg) addr,
            options(nostack, preserves_flags)
        );
    }
}

/// Unmask IRQ/FIQ in DAIF. Kept separate from [`install_vectors`] so the
/// scheduler can defer enabling interrupts until the timer is programmed.
pub fn enable_irqs() {
    // SAFETY: privileged but always safe at EL1.
    unsafe {
        core::arch::asm!("msr daifclr, #0x3", options(nomem, nostack));
    }
}

/// Mask IRQ/FIQ in DAIF.
pub fn disable_irqs() {
    // SAFETY: privileged but always safe at EL1.
    unsafe {
        core::arch::asm!("msr daifset, #0x3", options(nomem, nostack));
    }
}

#[no_mangle]
extern "C" fn hyperion_trap_unhandled(tf: &TrapFrame, kind: u64) -> ! {
    let label = match kind {
        0 => "synchronous from SP_EL0",
        1 => "SError (kernel)",
        2 => "SError (user)",
        _ => "unknown",
    };
    panic!(
        "unhandled exception: {label}\n  ELR={:#x} ESR={:#x} FAR={:#x} SPSR={:#x}",
        tf.elr, tf.esr, tf.far, tf.spsr
    );
}

#[no_mangle]
extern "C" fn hyperion_trap_sync(tf: &mut TrapFrame) {
    // ESR_EL1 EC field (bits 31:26) tells us what triggered the trap.
    let ec = (tf.esr >> 26) & 0x3f;
    match ec {
        // SVC from aarch64 -> syscall.
        0x15 => {
            let nr = tf.regs[8];
            let args = [
                tf.regs[0], tf.regs[1], tf.regs[2], tf.regs[3], tf.regs[4], tf.regs[5],
            ];
            let ret = crate::syscall::dispatch(nr, args);
            tf.regs[0] = ret as u64;
        }
        // Brk -> debug breakpoint, treat as panic.
        0x3c => panic!("BRK at {:#x}", tf.elr),
        _ => panic!(
            "unhandled sync exception EC={ec:#x} ELR={:#x} ESR={:#x} FAR={:#x}",
            tf.elr, tf.esr, tf.far
        ),
    }
}

#[no_mangle]
extern "C" fn hyperion_trap_irq(_tf: &mut TrapFrame) {
    super::gic::handle_irq();
}
