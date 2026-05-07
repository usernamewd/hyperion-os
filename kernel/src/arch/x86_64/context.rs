//! Thread context switch on x86_64.
//!
//! We save the System V AMD64 ABI *callee-saved* registers
//! (`rbx`, `rbp`, `r12..r15`, `rsp`) into a [`Context`] and restore the
//! next thread's. RIP is implicit: `__ctx_switch` ends with `ret`, so
//! the next thread resumes at whatever return address its `rsp`
//! points at.
//!
//! For brand-new threads we fake a tiny stack frame: at `rsp` we
//! place the address of [`__thread_trampoline`], and stash the
//! entrypoint and its argument into callee-saved registers
//! (`r15` = entry, `r14` = arg). The trampoline then calls `entry(arg)`
//! and falls through to `hyperion_thread_exit` if it returns.

use core::arch::global_asm;

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct Context {
    pub rbx: u64, // 0
    pub rbp: u64, // 8
    pub r12: u64, // 16
    pub r13: u64, // 24
    pub r14: u64, // 32
    pub r15: u64, // 40
    pub rsp: u64, // 48
}

global_asm!(
    r#"
    .section .text
    .globl __ctx_switch
    .type __ctx_switch, @function
__ctx_switch:
    // rdi = &prev->ctx, rsi = &next->ctx
    mov     [rdi + 0],  rbx
    mov     [rdi + 8],  rbp
    mov     [rdi + 16], r12
    mov     [rdi + 24], r13
    mov     [rdi + 32], r14
    mov     [rdi + 40], r15
    mov     [rdi + 48], rsp

    mov     rsp, [rsi + 48]
    mov     r15, [rsi + 40]
    mov     r14, [rsi + 32]
    mov     r13, [rsi + 24]
    mov     r12, [rsi + 16]
    mov     rbp, [rsi + 8]
    mov     rbx, [rsi + 0]
    ret
    .size __ctx_switch, . - __ctx_switch
"#
);

extern "C" {
    /// Save the current context into `*prev` and restore from `*next`.
    /// On return, the caller is now executing on `next`'s stack.
    pub fn __ctx_switch(prev: *mut Context, next: *const Context);
}

global_asm!(
    r#"
    .section .text
    .globl __thread_trampoline
    .type __thread_trampoline, @function
__thread_trampoline:
    // r15 = entry, r14 = arg (set up by init_context).
    mov     rdi, r14
    call    r15
    // entry returned (it shouldn't, since it's `-> !`) — fall through
    // to scheduler exit so the thread is removed from the runqueue.
    call    hyperion_thread_exit
2:  hlt
    jmp     2b
    .size __thread_trampoline, . - __thread_trampoline
"#
);

extern "C" {
    /// Symbol referenced by the trampoline above.
    fn __thread_trampoline();
}

/// Build an initial context that, when switched to, will start executing
/// `entry(arg)` on `stack_top`.
///
/// We mutate the top of the supplied stack so that the first `ret` in
/// `__ctx_switch` pops [`__thread_trampoline`] off it and lands there.
pub fn init_context(
    ctx: &mut Context,
    entry: extern "C" fn(usize) -> !,
    arg: usize,
    stack_top: usize,
) {
    // The SysV ABI requires `rsp % 16 == 0` *before* a `call`; after a
    // `call`, the function entry sees `rsp % 16 == 8`. Our trampoline
    // is reached via `ret`, which pops the return address — so we need
    // `rsp % 16 == 0` post-ret. Place the trampoline pointer at
    // `aligned - 8` and set `rsp = aligned - 8`. After ret pops it,
    // rsp == aligned, which is 16-aligned.
    let aligned_top = stack_top & !0xf;
    let slot = (aligned_top - 8) as *mut u64;
    // SAFETY: `slot` is the top word of a heap-allocated stack owned
    // by the calling Thread; nothing else accesses it before the
    // first context switch into this thread.
    unsafe {
        slot.write(__thread_trampoline as usize as u64);
    }
    ctx.rsp = (aligned_top - 8) as u64;
    ctx.r15 = (entry as usize) as u64;
    ctx.r14 = arg as u64;
    ctx.rbx = 0;
    ctx.rbp = 0;
    ctx.r12 = 0;
    ctx.r13 = 0;
}

#[no_mangle]
extern "C" fn hyperion_thread_exit() -> ! {
    crate::log::warn!("thread returned; halting it");
    crate::proc::scheduler::exit_current();
}
