//! Processes (address-space + capability container).

use alloc::collections::BTreeMap;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mm::vmm::AddressSpace;
use crate::sync::Mutex;

/// Process identifier.
pub type Pid = u64;

/// A process: an address space + a capability table + threads.
pub struct Process {
    pub pid: Pid,
    pub name: String,
    pub aspace: AddressSpace,
}

impl Process {
    pub fn new_kernel(name: &str) -> Self {
        Self {
            pid: alloc_pid(),
            name: String::from(name),
            aspace: AddressSpace::new_unmaterialised(),
        }
    }
}

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

fn alloc_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

static PROCESSES: Mutex<BTreeMap<Pid, Process>> = Mutex::new(BTreeMap::new());

/// One-time process subsystem init: creates the kernel process (PID 1).
pub fn init() {
    let kernel = Process::new_kernel("kernel");
    PROCESSES.lock().insert(kernel.pid, kernel);
}

/// Return the kernel process PID (the first one ever created).
pub fn kernel_pid() -> Pid {
    *PROCESSES
        .lock()
        .keys()
        .next()
        .expect("kernel process exists")
}

/// Snapshot of (pid, name) pairs for the shell `ps` command.
pub fn list() -> alloc::vec::Vec<(Pid, alloc::string::String)> {
    PROCESSES
        .lock()
        .iter()
        .map(|(p, proc)| (*p, proc.name.clone()))
        .collect()
}
