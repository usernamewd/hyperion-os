//! Minimal GICv2 driver for QEMU `virt`.
//!
//! Distributor:    0x0800_0000
//! CPU interface:  0x0801_0000
//!
//! We only need: enable PPIs, acknowledge IRQs, signal EOI. SPI / SGI
//! routing is left as future work.

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER: usize = 0x100;

const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00c;
const GICC_EOIR: usize = 0x010;

#[inline(always)]
unsafe fn dist_w(off: usize, v: u32) {
    // SAFETY: distributor MMIO mapped identity.
    unsafe { core::ptr::write_volatile((GICD_BASE + off) as *mut u32, v) };
}

#[inline(always)]
unsafe fn cpu_w(off: usize, v: u32) {
    // SAFETY: CPU IF MMIO mapped identity.
    unsafe { core::ptr::write_volatile((GICC_BASE + off) as *mut u32, v) };
}

#[inline(always)]
unsafe fn cpu_r(off: usize) -> u32 {
    // SAFETY: CPU IF MMIO mapped identity.
    unsafe { core::ptr::read_volatile((GICC_BASE + off) as *const u32) }
}

/// Bring up the GIC: enable distributor + CPU interface, lower priority mask.
pub fn init() {
    // SAFETY: single-CPU init at boot.
    unsafe {
        dist_w(GICD_CTLR, 1);
        cpu_w(GICC_PMR, 0xff);
        cpu_w(GICC_CTLR, 1);
    }
}

/// Enable a private peripheral interrupt (PPI) by INTID (16..=31).
pub fn enable_ppi(intid: u32) {
    debug_assert!((16..32).contains(&intid));
    let reg = (intid / 32) as usize;
    let bit = intid % 32;
    // SAFETY: distributor MMIO mapped identity; one-shot enable.
    unsafe {
        dist_w(GICD_ISENABLER + reg * 4, 1u32 << bit);
    }
}

/// Called from the IRQ trap. Reads the active interrupt, dispatches it,
/// then signals end-of-interrupt.
pub fn handle_irq() {
    // SAFETY: GICC mapped identity.
    let iar = unsafe { cpu_r(GICC_IAR) };
    let intid = iar & 0x3ff;
    if intid == 1023 {
        // Spurious, no work to do.
        return;
    }

    if super::timer::is_timer_irq(intid) {
        super::timer::rearm();
        crate::proc::scheduler::on_tick();
    } else {
        crate::log::warn!("unrouted IRQ {}", intid);
    }

    // SAFETY: writing EOIR is mandatory to deassert the interrupt.
    unsafe { cpu_w(GICC_EOIR, iar) };
}
