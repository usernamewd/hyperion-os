//! Threads of execution.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::context::{init_context, Context};

/// Thread identifier.
pub type Tid = u64;

/// Lifecycle state of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

const STACK_SIZE: usize = 16 * 1024; // 16 KiB

/// A schedulable thread. The kernel-resident stack is heap-allocated; in a
/// future EL0 world this would migrate to a userspace mapping.
pub struct Thread {
    pub tid: Tid,
    pub name: String,
    pub state: ThreadState,
    pub ctx: Context,
    pub pid: super::Pid,
    /// Owned kernel stack.
    _stack: Box<[u8]>,
}

static NEXT_TID: AtomicU64 = AtomicU64::new(1);

fn alloc_tid() -> Tid {
    NEXT_TID.fetch_add(1, Ordering::Relaxed)
}

impl Thread {
    /// Spawn a new ready thread that will run `entry(arg)`.
    pub fn new(name: &str, pid: super::Pid, entry: extern "C" fn(usize) -> !, arg: usize) -> Self {
        let mut stack: Vec<u8> = vec![0u8; STACK_SIZE];
        let stack_top = stack.as_mut_ptr() as usize + STACK_SIZE;
        let stack: Box<[u8]> = stack.into_boxed_slice();
        let mut ctx = Context::default();
        init_context(&mut ctx, entry, arg, stack_top);
        Self {
            tid: alloc_tid(),
            name: String::from(name),
            state: ThreadState::Ready,
            ctx,
            pid,
            _stack: stack,
        }
    }

    /// Construct a sentinel thread that represents "currently executing
    /// hand-built kernel context" — used during boot so we have a valid
    /// previous-thread pointer for the very first `__ctx_switch`.
    pub fn boot_thread(pid: super::Pid) -> Self {
        Self {
            tid: alloc_tid(),
            name: String::from("boot"),
            state: ThreadState::Running,
            ctx: Context::default(),
            pid,
            _stack: Vec::new().into_boxed_slice(),
        }
    }
}
