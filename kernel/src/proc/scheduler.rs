//! Round-robin preemptive scheduler.
//!
//! Single CPU for now. The scheduler maintains a runqueue of `Thread`s.
//! Timer ticks (10 ms) call [`on_tick`] which marks the current thread for
//! a reschedule; the actual context switch happens on the next call to
//! [`yield_now`] or at the end of an IRQ handler.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;

use super::context::__ctx_switch;
use super::thread::{Thread, ThreadState, Tid};
use crate::sync::{IrqLock, Mutex};

struct Sched {
    /// Currently running thread. Boxed so its address is stable.
    current: Option<Box<Thread>>,
    /// Ready queue.
    ready: VecDeque<Box<Thread>>,
    /// Set by the timer interrupt; cleared after a reschedule.
    need_resched: bool,
}

impl Sched {
    const fn new() -> Self {
        Self {
            current: None,
            ready: VecDeque::new(),
            need_resched: false,
        }
    }
}

static SCHED: Mutex<Sched> = Mutex::new(Sched::new());

/// One-time init: install a "boot" thread as `current` so the very first
/// context switch has a valid previous context to save into.
pub fn init() {
    let mut s = SCHED.lock();
    let pid = super::process::kernel_pid();
    s.current = Some(Box::new(Thread::boot_thread(pid)));
}

/// Push a new ready thread onto the runqueue.
pub fn spawn(name: &str, entry: extern "C" fn(usize) -> !, arg: usize) -> Tid {
    let pid = super::process::kernel_pid();
    let t = Box::new(Thread::new(name, pid, entry, arg));
    let tid = t.tid;
    SCHED.lock().ready.push_back(t);
    tid
}

/// Called from the timer IRQ; schedules a reschedule on next safe point.
pub fn on_tick() {
    SCHED.lock().need_resched = true;
}

/// Voluntarily yield the CPU to another ready thread. If no other thread
/// is ready, returns immediately.
pub fn yield_now() {
    // Mask IRQs around the runqueue manipulation; the actual ctx switch
    // happens with IRQs masked too — they are re-enabled on the next
    // thread's stack via the trampoline / earlier DAIF state.
    let _g = IrqLock::new();
    let (prev_ctx, next_ctx) = {
        let mut s = SCHED.lock();
        s.need_resched = false;

        let mut next = match s.ready.pop_front() {
            Some(t) => t,
            None => return, // no work
        };
        // Move current -> ready, next -> current.
        let mut current = s.current.take().expect("current thread");
        current.state = ThreadState::Ready;
        let prev_ctx_ptr: *mut super::context::Context = &mut current.ctx;
        next.state = ThreadState::Running;
        let next_ctx_ptr: *const super::context::Context = &next.ctx;
        s.ready.push_back(current);
        s.current = Some(next);
        (prev_ctx_ptr, next_ctx_ptr)
    };
    // SAFETY: prev/next contexts point into Boxed Threads owned by SCHED.
    // We dropped the Mutex guard before switching, so the lock is free
    // again on the other side.
    unsafe { __ctx_switch(prev_ctx, next_ctx) };
}

/// Run the scheduler loop forever. The first time this is called it
/// effectively becomes the idle thread on the boot CPU.
pub fn run() -> ! {
    crate::arch::aarch64::exceptions::enable_irqs();
    loop {
        // If there's work to do, switch to it; otherwise sleep waiting
        // for an interrupt.
        let has_work = !SCHED.lock().ready.is_empty();
        if has_work {
            yield_now();
        } else {
            // SAFETY: WFI just stalls until an interrupt is pending.
            unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
        }
    }
}

/// Mark the current thread as exited and yield permanently.
pub fn exit_current() -> ! {
    {
        let mut s = SCHED.lock();
        if let Some(t) = s.current.as_mut() {
            t.state = ThreadState::Exited;
            crate::log::info!("thread {} exited", t.tid);
        }
    }
    // Drop the current thread by replacing with the next ready, never
    // re-enqueueing.
    let _g = IrqLock::new();
    let next_ctx = {
        let mut s = SCHED.lock();
        let mut next = s
            .ready
            .pop_front()
            .expect("must have something to switch to (idle missing?)");
        next.state = ThreadState::Running;
        let p: *const super::context::Context = &next.ctx;
        s.current = Some(next);
        p
    };
    let mut tossed = super::context::Context::default();
    // SAFETY: we never come back to `tossed`.
    unsafe { __ctx_switch(&mut tossed as *mut _, next_ctx) };
    unreachable!()
}

/// Snapshot of (tid, name, state) for the shell `ps` command.
pub fn snapshot() -> alloc::vec::Vec<(Tid, String, ThreadState)> {
    let s = SCHED.lock();
    let mut v = alloc::vec::Vec::new();
    if let Some(c) = s.current.as_ref() {
        v.push((c.tid, c.name.clone(), c.state));
    }
    for t in &s.ready {
        v.push((t.tid, t.name.clone(), t.state));
    }
    v
}
