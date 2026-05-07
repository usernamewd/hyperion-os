//! Local APIC driver — xAPIC mode.
//!
//! We use the LAPIC for two things:
//!
//! 1. **Timer.** A periodic LAPIC timer programmed in
//!    [`crate::arch::x86_64::timer::init`] fires the scheduler tick
//!    on vector `0x20`.
//! 2. **EOI.** Acknowledging external interrupts that came in via the
//!    I/O APIC (which routes legacy ISA / PCI IRQs to LAPIC vectors).
//!
//! We always run xAPIC (memory-mapped at 0xFEE0_0000 on every PC since
//! the Pentium Pro). x2APIC is a follow-up and would only be wired in
//! once we parse ACPI MADT to discover the LAPIC mode preferred by the
//! firmware.

use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

const REG_ID: usize = 0x20;
const REG_TPR: usize = 0x80;
const REG_EOI: usize = 0xB0;
const REG_LDR: usize = 0xD0;
const REG_DFR: usize = 0xE0;
const REG_SVR: usize = 0xF0;
const REG_LVT_TIMER: usize = 0x320;
const REG_LVT_LINT0: usize = 0x350;
const REG_LVT_LINT1: usize = 0x360;
const REG_LVT_ERR: usize = 0x370;
const REG_TIMER_INITIAL: usize = 0x380;
const REG_TIMER_CURRENT: usize = 0x390;
const REG_TIMER_DIVIDE: usize = 0x3E0;

const SVR_ENABLE: u32 = 0x100;
const SPURIOUS_VECTOR: u32 = 0xFF;

const LVT_MASKED: u32 = 1 << 16;
const LVT_PERIODIC: u32 = 1 << 17;

/// LAPIC MMIO base, populated from BootInfo at init time.
static LAPIC_BASE: AtomicU64 = AtomicU64::new(0);

unsafe fn read(base: u64, reg: usize) -> u32 {
    // SAFETY: caller guarantees `base` is the mapped LAPIC.
    unsafe { ptr::read_volatile((base as usize + reg) as *const u32) }
}

unsafe fn write(base: u64, reg: usize, val: u32) {
    // SAFETY: caller guarantees `base` is the mapped LAPIC.
    unsafe { ptr::write_volatile((base as usize + reg) as *mut u32, val) }
}

/// Initialise the LAPIC: enable, set the spurious vector, mask LINT0
/// and LINT1 (we don't use them), and clear the error LVT.
pub fn init() {
    let bi = crate::hal::info();
    let base = bi.intc.primary.base;
    LAPIC_BASE.store(base, Ordering::Release);

    // SAFETY: BootInfo asserts the LAPIC MMIO is mapped.
    unsafe {
        // TPR = 0 (accept every priority).
        write(base, REG_TPR, 0);
        // Logical destination = 0xFF (broadcast in flat mode).
        write(base, REG_DFR, 0xFFFF_FFFF);
        write(base, REG_LDR, 0x0100_0000);
        // Mask LVT entries we don't use.
        write(base, REG_LVT_LINT0, LVT_MASKED);
        write(base, REG_LVT_LINT1, LVT_MASKED);
        // Error vector ends up unused but installed at 0xFE.
        write(base, REG_LVT_ERR, 0xFE);
        // Spurious vector + enable.
        write(base, REG_SVR, SVR_ENABLE | SPURIOUS_VECTOR);
    }
}

/// Acknowledge the in-service interrupt. Must be the last write of an
/// IRQ handler; otherwise the LAPIC won't deliver another at the same
/// or lower priority.
#[inline]
pub fn eoi() {
    let base = LAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    // SAFETY: LAPIC is mapped once `init` has run.
    unsafe { write(base, REG_EOI, 0) };
}

/// Configure the LAPIC timer in periodic mode at the given vector,
/// firing every `interval_ticks` of the LAPIC timer's bus tick.
pub fn init_timer(vector: u8, interval_ticks: u32) {
    let base = LAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    // SAFETY: LAPIC is mapped.
    unsafe {
        // Divide by 16 — the conventional default; gives us ~1 tick /
        // µs on QEMU's emulated bus.
        write(base, REG_TIMER_DIVIDE, 0b0011);
        write(base, REG_LVT_TIMER, LVT_PERIODIC | (vector as u32));
        write(base, REG_TIMER_INITIAL, interval_ticks);
    }
}

#[allow(dead_code)]
pub fn timer_current() -> u32 {
    let base = LAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    // SAFETY: LAPIC is mapped.
    unsafe { read(base, REG_TIMER_CURRENT) }
}

#[allow(dead_code)]
pub fn id() -> u32 {
    let base = LAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    // SAFETY: LAPIC is mapped.
    unsafe { read(base, REG_ID) >> 24 }
}
