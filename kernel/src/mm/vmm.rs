//! Virtual memory manager.
//!
//! The kernel still boots identity-mapped, but processes now own a concrete
//! address-space object that records page-granular virtual mappings. Hardware
//! page-table backends can consume the same mapping list when EL0 switching is
//! enabled.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::mm::address::{PhysAddr, VirtAddr};

const PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    ZeroLength,
    Unaligned,
    LengthOverflow,
    Overlap,
    OutOfMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub va: VirtAddr,
    pub pa: PhysAddr,
    pub len: usize,
}

/// A virtual address space owned by a process.
#[derive(Debug)]
pub struct AddressSpace {
    /// Root page-table frame (PA). Zero if the AS has not been materialised.
    pub root: PhysAddr,
    mappings: BTreeMap<u64, Mapping>,
}

impl AddressSpace {
    pub fn new_unmaterialised() -> Self {
        Self {
            root: PhysAddr::new(0),
            mappings: BTreeMap::new(),
        }
    }

    pub fn map(&mut self, va: VirtAddr, pa: PhysAddr, len: usize) -> Result<(), MapError> {
        let len = normalise_len(len)?;
        ensure_page_aligned(va.raw())?;
        ensure_page_aligned(pa.raw())?;
        let end = checked_end(va.raw(), len)?;

        for existing in self.mappings.values() {
            let start = existing.va.raw();
            let existing_end = checked_end(start, existing.len)?;
            if ranges_overlap(va.raw(), end, start, existing_end) {
                return Err(MapError::Overlap);
            }
        }

        self.ensure_root()?;
        self.mappings.insert(va.raw(), Mapping { va, pa, len });
        Ok(())
    }

    pub fn unmap(&mut self, va: VirtAddr) -> Option<Mapping> {
        self.mappings.remove(&va.raw())
    }

    pub fn translate(&self, va: VirtAddr) -> Option<PhysAddr> {
        let (start, mapping) = self.mappings.range(..=va.raw()).next_back()?;
        let offset = va.raw().checked_sub(*start)?;
        if offset < mapping.len as u64 {
            Some(PhysAddr::new(mapping.pa.raw() + offset))
        } else {
            None
        }
    }

    pub fn mappings(&self) -> Vec<Mapping> {
        self.mappings.values().copied().collect()
    }

    fn ensure_root(&mut self) -> Result<(), MapError> {
        if self.root.raw() != 0 {
            return Ok(());
        }
        let frame = crate::mm::pmm::alloc_frame().ok_or(MapError::OutOfMemory)?;
        unsafe {
            core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE as usize);
        }
        self.root = PhysAddr::new(frame as u64);
        Ok(())
    }
}

fn normalise_len(len: usize) -> Result<usize, MapError> {
    if len == 0 {
        return Err(MapError::ZeroLength);
    }
    let len = len as u64;
    let rounded = len
        .checked_add(PAGE_SIZE - 1)
        .ok_or(MapError::LengthOverflow)?
        & !(PAGE_SIZE - 1);
    usize::try_from(rounded).map_err(|_| MapError::LengthOverflow)
}

fn ensure_page_aligned(addr: u64) -> Result<(), MapError> {
    if addr & (PAGE_SIZE - 1) == 0 {
        Ok(())
    } else {
        Err(MapError::Unaligned)
    }
}

fn checked_end(start: u64, len: usize) -> Result<u64, MapError> {
    start
        .checked_add(len as u64)
        .ok_or(MapError::LengthOverflow)
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

/// One-time VMM initialisation.
pub fn init() {}
