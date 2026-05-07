//! Virtual memory manager (placeholder).
//!
//! Today the kernel runs identity-mapped with the MMU off. The plumbing in
//! [`crate::arch::aarch64::mmu`] is ready to be turned on; once we add EL0
//! user processes this module will own the per-process [`AddressSpace`] and
//! drive the page-table updates.

use crate::mm::address::{PhysAddr, VirtAddr};

/// A virtual address space owned by a process.
#[derive(Debug)]
pub struct AddressSpace {
    /// Root of the L1 page table (PA). Zero if the AS has not been
    /// materialised yet.
    pub root: PhysAddr,
}

impl AddressSpace {
    pub fn new_unmaterialised() -> Self {
        Self {
            root: PhysAddr::new(0),
        }
    }

    /// Map `va -> pa` for `len` bytes with the given attributes.
    /// Currently a stub; will populate the L1/L2/L3 tables once we turn
    /// on per-process VAs.
    pub fn map(&mut self, _va: VirtAddr, _pa: PhysAddr, _len: usize) {
        // intentionally empty
    }
}

/// One-time VMM initialisation. Currently no-op because the kernel runs
/// identity-mapped; called for symmetry with the rest of the boot path.
pub fn init() {}
