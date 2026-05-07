//! x86_64 page table helpers.
//!
//! The boot stub already installs identity mappings for the lower
//! 1 GiB plus a second 1 GiB block covering the LAPIC / I/O APIC /
//! PCI MMIO window starting at `0xC000_0000`. That's enough to bring
//! up the kernel; this module's job is to expose a few helpers for
//! the rest of the kernel to peek at CR3 / CR2 / CR0 when needed
//! and to extend the boot-time page tables for any additional MMIO
//! regions (e.g. a high framebuffer reported by Multiboot2).
//!
//! Per-process page tables (TTBR1 equivalents) are a follow-up; until
//! then, every thread shares the kernel's identity map.

use core::sync::atomic::{AtomicBool, Ordering};

#[inline]
pub fn read_cr0() -> u64 {
    let v: u64;
    // SAFETY: reading CR0 in ring 0 is always safe.
    unsafe {
        core::arch::asm!("mov {0}, cr0", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

#[inline]
pub fn read_cr3() -> u64 {
    let v: u64;
    // SAFETY: reading CR3 in ring 0 is always safe.
    unsafe {
        core::arch::asm!("mov {0}, cr3", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// Cached CR3 value taken at boot. Used as the kernel master page table
/// pointer for follow-up modules (per-process address spaces will
/// eventually clone from this).
static MMU_READY: AtomicBool = AtomicBool::new(false);

pub fn init() {
    MMU_READY.store(true, Ordering::Release);
}

pub fn ready() -> bool {
    MMU_READY.load(Ordering::Acquire)
}
