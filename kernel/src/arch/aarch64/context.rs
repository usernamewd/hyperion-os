//! Thread context switch on aarch64.
//!
//! We save the AArch64 *callee-saved* registers (`x19..x30`, `sp`, plus
//! the frame pointer `x29`) into a [`Context`] and restore the next
//! thread's. The actual switch is a tiny piece of assembly so we have
//! full control over register layout.
//!
//! `x30` holds the return address and is written by the caller of
//! `switch`; on first entry we fake it to point at a small trampoline
//! that calls a thread's entry function and then exits cleanly.

use core::arch::global_asm;

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct Context {
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub fp: u64, // x29
    pub lr: u64, // x30
    pub sp: u64,
}

global_asm!(
    r#"
    .section .text
    .globl __ctx_switch
    .type __ctx_switch, @function
__ctx_switch:
    // x0 = &prev->ctx, x1 = &next->ctx
    stp     x19, x20, [x0, #0]
    stp     x21, x22, [x0, #16]
    stp     x23, x24, [x0, #32]
    stp     x25, x26, [x0, #48]
    stp     x27, x28, [x0, #64]
    stp     x29, x30, [x0, #80]
    mov     x9, sp
    str     x9,  [x0, #96]

    ldr     x9,  [x1, #96]
    mov     sp, x9
    ldp     x29, x30, [x1, #80]
    ldp     x27, x28, [x1, #64]
    ldp     x25, x26, [x1, #48]
    ldp     x23, x24, [x1, #32]
    ldp     x21, x22, [x1, #16]
    ldp     x19, x20, [x1, #0]
    ret
    .size __ctx_switch, . - __ctx_switch
"#
);

extern "C" {
    /// Save the current context into `*prev` and restore from `*next`.
    /// On return, the caller is now executing on `next`'s stack.
    pub fn __ctx_switch(prev: *mut Context, next: *const Context);
}

/// Build an initial context that, when switched to, will start executing
/// `entry(arg)` on `stack_top`.
pub fn init_context(
    ctx: &mut Context,
    entry: extern "C" fn(usize) -> !,
    arg: usize,
    stack_top: usize,
) {
    ctx.x19 = (entry as usize) as u64;
    ctx.x20 = arg as u64;
    ctx.lr = (__thread_trampoline as usize) as u64;
    // Round down to 16-byte alignment as required by AArch64.
    ctx.sp = (stack_top & !0xf) as u64;
    ctx.fp = 0;
}

global_asm!(
    r#"
    .section .text
    .globl __thread_trampoline
    .type __thread_trampoline, @function
__thread_trampoline:
    mov     x0, x20
    blr     x19
    // entry is `-> !`, but in case it returns, fall into thread_exit.
    bl      hyperion_thread_exit
1:  wfe
    b       1b
    .size __thread_trampoline, . - __thread_trampoline
"#
);

extern "C" {
    /// Symbol referenced by the trampoline above.
    fn __thread_trampoline();
}

#[no_mangle]
extern "C" fn hyperion_thread_exit() -> ! {
    crate::log::warn!("thread returned; halting it");
    crate::proc::scheduler::exit_current();
}
