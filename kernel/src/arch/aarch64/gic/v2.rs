//! GICv2 driver.
//!
//! Two memory-mapped frames: a Distributor (`gicd`) shared by all
//! cores, and a CPU Interface (`gicc`) that each core sees as its own.
//! On QEMU `virt`:
//!
//! * `gicd` = 0x0800_0000, 64 KiB
//! * `gicc` = 0x0801_0000, 8 KiB

use crate::sync::Mutex;

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER: usize = 0x100;

const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00c;
const GICC_EOIR: usize = 0x010;

#[derive(Default)]
struct State {
    gicd: usize,
    gicc: usize,
}

static STATE: Mutex<State> = Mutex::new(State { gicd: 0, gicc: 0 });

#[inline(always)]
unsafe fn w32(addr: usize, v: u32) {
    // SAFETY: caller guarantees `addr` is mapped MMIO.
    unsafe { core::ptr::write_volatile(addr as *mut u32, v) };
}

#[inline(always)]
unsafe fn r32(addr: usize) -> u32 {
    // SAFETY: caller guarantees `addr` is mapped MMIO.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Bring the controller up. Single-CPU, group-0/1 grouping is left at
/// the firmware default (everything in group 1 NS on QEMU virt is
/// fine — that's what Linux uses too).
///
/// # Safety
/// `gicd` and `gicc` must be valid identity-mapped MMIO regions for the
/// GICv2 distributor / CPU-interface respectively.
pub unsafe fn init(gicd: usize, gicc: usize) {
    {
        let mut s = STATE.lock();
        s.gicd = gicd;
        s.gicc = gicc;
    }
    // Enable the distributor.
    unsafe { w32(gicd + GICD_CTLR, 1) };
    // Lower priority mask so any priority can fire, then enable the CPU
    // interface for the boot CPU.
    unsafe { w32(gicc + GICC_PMR, 0xff) };
    unsafe { w32(gicc + GICC_CTLR, 1) };
}

/// Per-CPU bring-up for a secondary. The distributor is already
/// configured by [`init`]; each secondary just unmasks priorities and
/// enables its own CPU interface (`GICC_*` is banked per-CPU on v2).
pub fn init_per_cpu() {
    let s = STATE.lock();
    if s.gicc == 0 {
        return;
    }
    // SAFETY: gicc is mapped MMIO, banked per-CPU.
    unsafe {
        w32(s.gicc + GICC_PMR, 0xff);
        w32(s.gicc + GICC_CTLR, 1);
    }
}

/// Enable PPI `intid` (16..=31) at the distributor.
pub fn enable_ppi(intid: u32) {
    let s = STATE.lock();
    let reg = (intid / 32) as usize;
    let bit = intid % 32;
    // SAFETY: distributor mapped during init.
    unsafe { w32(s.gicd + GICD_ISENABLER + reg * 4, 1u32 << bit) };
}

/// Acknowledge the highest-priority pending interrupt and return its
/// INTID (or 1023 for spurious).
pub fn ack() -> u32 {
    let s = STATE.lock();
    // SAFETY: gicc mapped during init.
    let iar = unsafe { r32(s.gicc + GICC_IAR) };
    iar & 0x3ff
}

/// Signal end-of-interrupt for INTID `intid`. The IAR value is the same
/// number for v2; we just write it back.
pub fn eoi(intid: u32) {
    let s = STATE.lock();
    // SAFETY: gicc mapped during init.
    unsafe { w32(s.gicc + GICC_EOIR, intid) };
}
