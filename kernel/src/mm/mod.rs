//! Memory management.
//!
//! Three layers, brought up in order at boot:
//!
//! 1. [`pmm`]   — physical frame allocator (4 KiB pages, bitmap-backed).
//! 2. [`heap`]  — kernel heap (`linked_list_allocator`) installed as the
//!    `#[global_allocator]`. Backed by a 4 MiB region reserved in the
//!    linker script (`.heap`).
//! 3. [`vmm`]   — page-table builder (currently identity-mapping, see
//!    [`crate::arch::aarch64::mmu`]).
//!
//! Address types live in [`address`].

pub mod address;
pub mod heap;
pub mod pmm;
pub mod vmm;

pub use address::{PhysAddr, VirtAddr};

/// Initialise the memory subsystem. Must be called once on the boot CPU.
pub fn init() {
    pmm::init();
    heap::init();
    vmm::init();
}

/// Total / free RAM stats, primarily used by the shell `mem` command.
#[derive(Debug, Clone, Copy)]
pub struct MemStats {
    pub total_bytes: usize,
    pub free_bytes: usize,
    pub heap_total: usize,
    pub heap_used: usize,
}

/// Snapshot current memory stats.
pub fn stats() -> MemStats {
    MemStats {
        total_bytes: pmm::total_bytes(),
        free_bytes: pmm::free_bytes(),
        heap_total: heap::total_bytes(),
        heap_used: heap::used_bytes(),
    }
}
