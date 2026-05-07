//! Processes, threads, and the scheduler.
//!
//! Hyperion uses an L4-style separation between **address spaces**
//! (`process::Process` owns one) and **threads of execution**
//! (`thread::Thread`). The scheduler is round-robin and preemptive,
//! driven by the EL1 physical timer (see [`crate::arch::aarch64::timer`]).
//!
//! For now every thread executes at EL1 in the kernel address space. The
//! data structures and context-switch path are written so that lifting
//! threads to EL0 with their own page tables only requires hooking the
//! VMM in.

pub mod context;
pub mod percpu;
pub mod process;
pub mod scheduler;
pub mod thread;
#[cfg(target_arch = "aarch64")]
pub mod user;

pub use process::{Pid, Process};
pub use thread::{Thread, ThreadState, Tid};

/// Initialise the process subsystem.
pub fn init() {
    process::init();
    scheduler::init();
}
