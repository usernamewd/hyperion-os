//! Per-CPU state and online-CPU bookkeeping.
//!
//! Each CPU that has been brought online (boot CPU + secondaries woken
//! via PSCI CPU_ON on aarch64, AP startup via INIT-SIPI-SIPI on x86) has
//! a logical id assigned by the boot CPU and an entry in the [`PerCpu`]
//! table. The boot CPU is always logical id 0.
//!
//! On aarch64 the cpu id lives in TPIDR_EL1, which is otherwise unused
//! by the kernel and is privileged-only so EL0 user code can't see it.
//! On x86_64 we currently only run a single CPU, but the same accessor
//! is provided so cross-arch code reads `current_cpu_id()` without
//! `cfg`.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::sync::Mutex;

/// Maximum number of CPUs we track. Bumping this just costs static
/// bytes; QEMU virt happily supports up to 8 in the default
/// `-smp 8` configuration.
pub const MAX_CPUS: usize = 8;

/// Bitmask of CPUs that have called [`mark_online`]. Bit N == CPU N.
static ONLINE_MASK: AtomicU64 = AtomicU64::new(0);

/// Number of registered (booted) CPUs.
static REGISTERED: AtomicU32 = AtomicU32::new(0);

struct PerCpuSlot {
    #[allow(dead_code)]
    cpu_id: u32,
}

static PER_CPU: Mutex<[Option<PerCpuSlot>; MAX_CPUS]> =
    Mutex::new([None, None, None, None, None, None, None, None]);

/// Called from each CPU's boot path very early — once a stack is set up
/// but before we touch the heap. Stores the logical id in the per-CPU
/// register so subsequent code can find out who it is.
pub fn register_boot_cpu(cpu_id: u32) {
    let id = cpu_id as usize;
    if id >= MAX_CPUS {
        return;
    }
    write_self_id(cpu_id);
    let mut table = PER_CPU.lock();
    table[id] = Some(PerCpuSlot { cpu_id });
    REGISTERED.fetch_add(1, Ordering::Relaxed);
}

/// Mark a CPU as fully initialised + ready to take work.
pub fn mark_online(cpu_id: u32) {
    if cpu_id as usize >= MAX_CPUS {
        return;
    }
    ONLINE_MASK.fetch_or(1u64 << cpu_id, Ordering::Release);
}

/// Bitmask of currently-online CPUs.
pub fn online_mask() -> u64 {
    ONLINE_MASK.load(Ordering::Acquire)
}

/// Number of CPUs that have called [`mark_online`].
pub fn online_count() -> u32 {
    online_mask().count_ones()
}

/// Number of CPUs that have called [`register_boot_cpu`] but possibly
/// not yet [`mark_online`] — used during early boot for diagnostics.
pub fn registered_count() -> u32 {
    REGISTERED.load(Ordering::Relaxed)
}

/// Iterator-friendly snapshot of online CPU ids.
pub fn online_cpus() -> alloc::vec::Vec<u32> {
    let mask = online_mask();
    let mut v = alloc::vec::Vec::new();
    for i in 0..MAX_CPUS as u32 {
        if mask & (1u64 << i) != 0 {
            v.push(i);
        }
    }
    v
}

/// Read the current CPU's logical id.
#[inline]
pub fn current_cpu_id() -> u32 {
    read_self_id()
}

#[cfg(target_arch = "aarch64")]
fn write_self_id(id: u32) {
    let v: u64 = id as u64;
    // SAFETY: TPIDR_EL1 is privileged-only and dedicated to per-CPU use.
    unsafe {
        core::arch::asm!("msr tpidr_el1, {0}", in(reg) v, options(nomem, nostack));
    }
}

#[cfg(target_arch = "aarch64")]
fn read_self_id() -> u32 {
    let v: u64;
    // SAFETY: TPIDR_EL1 is privileged-only and dedicated to per-CPU use.
    unsafe {
        core::arch::asm!("mrs {0}, tpidr_el1", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v as u32
}

#[cfg(target_arch = "x86_64")]
fn write_self_id(_id: u32) {
    // x86_64 boots only the BSP for now; logical id is always 0 and
    // we don't bother with %gs:per-cpu yet. The lookup in
    // [`current_cpu_id`] returns 0 unconditionally below.
}

#[cfg(target_arch = "x86_64")]
fn read_self_id() -> u32 {
    0
}
