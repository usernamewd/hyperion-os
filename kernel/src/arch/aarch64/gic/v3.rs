//! GICv3 driver — the modern ARM interrupt controller.
//!
//! Two MMIO frames (Distributor + per-CPU Re-distributor) plus a CPU
//! interface that lives in *system registers* (ICC_*_EL1) instead of
//! MMIO. Almost every modern ARM server / dev board ships with a
//! GICv3 (Ampere Altra, Graviton, NXP LX2, Marvell ThunderX, the
//! Tianocore EDK II RPi4 build, QEMU `virt` with `-machine
//! gic-version=3`, …).
//!
//! We bring up only what we need for a uniprocessor preemptive
//! scheduler: enable group-1 non-secure interrupts, route them to this
//! core's CPU interface, and acknowledge them via system registers.
//! SPI routing / SMP wakeup are deferred until we add SMP.
//!
//! # Re-distributor layout
//!
//! For each CPU the re-distributor exposes two contiguous 64 KiB
//! frames:
//!
//! * `RD_base` — control / WAKER / TYPER
//! * `SGI_base` = `RD_base + 0x1_0000` — SGI/PPI configuration, where
//!   `GICR_ISENABLER0` (offset 0x100) lives.
//!
//! For now we assume CPU0's re-distributor is the first one — true on
//! QEMU virt and on any board where boot CPU is logical 0.

use core::arch::asm;

use crate::sync::Mutex;

const GICD_CTLR: usize = 0x000;

const GICR_WAKER: usize = 0x0014;
const GICR_WAKER_PROC_SLEEP: u32 = 1 << 1;
const GICR_WAKER_CHILDREN_ASLEEP: u32 = 1 << 2;

/// Offset of the SGI/PPI frame relative to the per-CPU re-distributor.
const GICR_SGI_OFF: usize = 0x1_0000;
const GICR_ISENABLER0: usize = 0x0100; // within SGI frame

#[derive(Default)]
struct State {
    gicd: usize,
    gicr: usize,
}

static STATE: Mutex<State> = Mutex::new(State { gicd: 0, gicr: 0 });

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

// Each ICC_*_EL1 sysreg has its own helper because aarch64 `msr` /
// `mrs` take the register name as an immediate, not a runtime value,
// so we can't share a single function.

#[inline(always)]
unsafe fn write_icc_sre_el1(v: u64) {
    // SAFETY: writing ICC_SRE_EL1 is privileged but always safe at EL1.
    unsafe { asm!("msr ICC_SRE_EL1, {0}; isb", in(reg) v) };
}

#[inline(always)]
unsafe fn write_icc_pmr_el1(v: u64) {
    unsafe { asm!("msr ICC_PMR_EL1, {0}", in(reg) v) };
}

#[inline(always)]
unsafe fn write_icc_igrpen1_el1(v: u64) {
    unsafe { asm!("msr ICC_IGRPEN1_EL1, {0}", in(reg) v) };
}

#[inline(always)]
unsafe fn read_icc_iar1_el1() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {0}, ICC_IAR1_EL1; isb", out(reg) v) };
    v
}

#[inline(always)]
unsafe fn write_icc_eoir1_el1(v: u64) {
    unsafe { asm!("msr ICC_EOIR1_EL1, {0}", in(reg) v) };
}

/// Bring the controller up.
///
/// # Safety
/// `gicd` must be the GICv3 Distributor base, `gicr` must be the
/// per-CPU Re-distributor base for *this* CPU.
pub unsafe fn init(gicd: usize, gicr: usize) {
    {
        let mut s = STATE.lock();
        s.gicd = gicd;
        s.gicr = gicr;
    }

    // 1. Wake the re-distributor for this CPU. Clear ProcessorSleep,
    //    spin until ChildrenAsleep clears too.
    // SAFETY: re-distributor mapped identity, we own it during boot.
    unsafe {
        let waker_addr = gicr + GICR_WAKER;
        let mut w = r32(waker_addr);
        w &= !GICR_WAKER_PROC_SLEEP;
        w32(waker_addr, w);
        // Spin until the rd considers itself awake.
        for _ in 0..1_000_000 {
            if r32(waker_addr) & GICR_WAKER_CHILDREN_ASLEEP == 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }

    // 2. Tell EL1 to use the system-register CPU interface, not the
    //    legacy memory-mapped one. ICC_SRE_EL1.SRE = 1.
    // SAFETY: ICC_SRE_EL1 is privileged but writing it is always safe.
    unsafe { write_icc_sre_el1(1) };

    // 3. Lower priority mask (any priority can preempt) and enable
    //    Group 1 non-secure interrupts at the CPU interface.
    // SAFETY: privileged sysregs, safe at EL1.
    unsafe {
        write_icc_pmr_el1(0xff);
        write_icc_igrpen1_el1(1);
    }

    // 4. Enable distributor. Set CTLR.EnableGrp1NS (bit 1) plus
    //    CTLR.ARE_NS (bit 4) so affinity routing is on. CTLR.RWP being
    //    polled by firmware/Linux is overkill for our single-core
    //    bring-up — we just set and forget.
    // SAFETY: distributor mapped identity.
    unsafe {
        let v = (1u32 << 1) | (1u32 << 4);
        w32(gicd + GICD_CTLR, v);
    }
}

/// Enable PPI `intid` (16..=31) at this CPU's re-distributor.
pub fn enable_ppi(intid: u32) {
    let s = STATE.lock();
    let bit = intid % 32; // PPIs/SGIs all live in ISENABLER0
                          // SAFETY: rd mapped during init.
    unsafe { w32(s.gicr + GICR_SGI_OFF + GICR_ISENABLER0, 1u32 << bit) };
}

/// Acknowledge the highest-priority pending interrupt for Group 1.
/// Returns the INTID (1023 for spurious).
pub fn ack() -> u32 {
    // SAFETY: privileged sysreg read, safe at EL1.
    let v = unsafe { read_icc_iar1_el1() };
    (v & 0x00ff_ffff) as u32
}

/// Signal end-of-interrupt for INTID `intid`.
pub fn eoi(intid: u32) {
    // SAFETY: privileged sysreg write, safe at EL1.
    unsafe { write_icc_eoir1_el1(intid as u64) };
}
