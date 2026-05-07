//! aarch64 MMU page table builder.
//!
//! We use 4KiB granules and a 39-bit virtual address space (3 levels of
//! page tables: L1 / L2 / L3). The kernel runs identity-mapped (VA == PA)
//! across the lower half (TTBR0_EL1); the upper half (TTBR1_EL1) is
//! reserved for future per-process kernel mappings.
//!
//! This module is currently used for documentation and a manually-invoked
//! `enable()` path; the kernel boots fine with the MMU off because all of
//! our MMIO and RAM accesses go through identity addresses. Turning the
//! MMU on is required before we can start running EL0 user processes —
//! that is wired up but gated behind a feature flag for now.

use core::sync::atomic::{AtomicBool, Ordering};

const PAGE_SIZE: usize = 4096;
const ENTRIES: usize = 512;

const DESC_VALID: u64 = 1 << 0;
const DESC_TABLE: u64 = 1 << 1;
const DESC_PAGE: u64 = 1 << 1;
const DESC_AF: u64 = 1 << 10;
const DESC_SH_INNER: u64 = 0b11 << 8;
const DESC_AP_RW_EL1: u64 = 0b00 << 6;
const DESC_AP_RW_EL1_RO: u64 = 0b10 << 6;

const ATTR_NORMAL_IDX: u64 = 0;
const ATTR_DEVICE_IDX: u64 = 1;

const MAIR_ATTR_NORMAL: u64 = 0xff;
const MAIR_ATTR_DEVICE: u64 = 0x04;

#[repr(C, align(4096))]
struct PageTable([u64; ENTRIES]);

static mut L1_TABLE: PageTable = PageTable([0; ENTRIES]);
static mut L2_TABLE: PageTable = PageTable([0; ENTRIES]);

static MMU_ENABLED: AtomicBool = AtomicBool::new(false);

/// Build a coarse identity map covering the first 1 GiB of the address
/// space (which on QEMU virt covers all MMIO + the start of RAM) using
/// 2 MiB block descriptors at L2.
///
/// # Safety
/// Must be called exactly once and before [`enable`].
pub unsafe fn build_identity_map() {
    // SAFETY: L1_TABLE and L2_TABLE are only accessed before MMU is enabled
    // and on the boot CPU. We use raw pointer manipulation to avoid the
    // `static_mut_refs` lint while preserving exclusivity.
    unsafe {
        let l1: *mut [u64; ENTRIES] = &raw mut L1_TABLE.0;
        let l2: *mut [u64; ENTRIES] = &raw mut L2_TABLE.0;

        for i in 0..ENTRIES {
            (*l1)[i] = 0;
            (*l2)[i] = 0;
        }

        // L1[0] -> L2 table
        let l2_pa = (&raw const L2_TABLE) as u64;
        (*l1)[0] = l2_pa | DESC_VALID | DESC_TABLE;

        for i in 0..ENTRIES {
            let pa: u64 = (i as u64) << 21; // 2 MiB blocks
                                            // First 0x4000_0000 is MMIO (UART, GIC, RTC, virtio-mmio, etc.).
            let attr_idx = if pa < 0x4000_0000 {
                ATTR_DEVICE_IDX
            } else {
                ATTR_NORMAL_IDX
            };
            (*l2)[i] = pa | (attr_idx << 2) | DESC_AF | DESC_SH_INNER | DESC_AP_RW_EL1 | DESC_VALID;
        }
    }
}

/// Enable the MMU. Must be called after [`build_identity_map`].
///
/// # Safety
/// Caller must ensure no other CPU is running and that the page tables are
/// fully populated.
pub unsafe fn enable() {
    if MMU_ENABLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let l1_pa = (&raw const L1_TABLE) as u64;

    // SAFETY: privileged register writes; we are at EL1 with all caches off.
    unsafe {
        // MAIR_EL1
        let mair = (MAIR_ATTR_NORMAL << (8 * ATTR_NORMAL_IDX))
            | (MAIR_ATTR_DEVICE << (8 * ATTR_DEVICE_IDX));
        core::arch::asm!("msr mair_el1, {0}", in(reg) mair);

        // TCR_EL1: T0SZ=25 (39-bit VA), 4KiB granule, inner+outer WB cacheable.
        // (TG0=4K is encoded as 0b00, so we omit that field.)
        let tcr: u64 = 25u64               // T0SZ
            | (25u64 << 16)                // T1SZ
            | (1u64 << 8)                  // IRGN0=WB
            | (1u64 << 10)                 // ORGN0=WB
            | (3u64 << 12)                 // SH0=Inner Shareable
            | (1u64 << 24)                 // IRGN1=WB
            | (1u64 << 26)                 // ORGN1=WB
            | (3u64 << 28)                 // SH1=Inner Shareable
            | (2u64 << 30)                 // TG1=4K
            | (1u64 << 32); // IPS=36-bit (64GB)
        core::arch::asm!("msr tcr_el1, {0}", in(reg) tcr);

        // TTBR0/1
        core::arch::asm!("msr ttbr0_el1, {0}", in(reg) l1_pa);
        core::arch::asm!("msr ttbr1_el1, {0}", in(reg) l1_pa);

        // ISB + DSB before enabling.
        core::arch::asm!("dsb sy; isb");

        // SCTLR_EL1: M=1 (MMU), C=1 (cache), I=1 (icache).
        let mut sctlr: u64;
        core::arch::asm!("mrs {0}, sctlr_el1", out(reg) sctlr);
        sctlr |= 1 | (1 << 2) | (1 << 12);
        core::arch::asm!("msr sctlr_el1, {0}; isb", in(reg) sctlr);
    }
}

/// Whether the MMU has been enabled by us.
pub fn enabled() -> bool {
    MMU_ENABLED.load(Ordering::Relaxed)
}

#[allow(dead_code)]
const _PAGE_SIZE_CHECK: () = assert!(PAGE_SIZE == 4096);

// suppress dead_code warnings from constants we keep for documentation
#[allow(dead_code)]
const _UNUSED_DESCRIPTOR_BITS: u64 = DESC_PAGE | DESC_AP_RW_EL1_RO;
