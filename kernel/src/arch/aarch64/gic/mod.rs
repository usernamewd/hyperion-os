//! Generic Interrupt Controller driver.
//!
//! Hyperion supports both legacy memory-mapped CPU interfaces (GICv2 —
//! GIC-400, Cortex-A15 GIC, the QEMU `virt` default) and the
//! system-register CPU interfaces (GICv3 — almost every recent ARM
//! server SoC: Ampere, Graviton, Apple Silicon's Apple Interrupt
//! Controller is GIC-shaped, the Tianocore RPi4 build, …).
//!
//! Which one we use is decided at boot from
//! [`crate::hal::BootInfo::gic`]. The dispatch happens through this
//! module so the rest of the kernel never sees the difference.

use crate::hal::boot_info::IntcKind;

mod v2;
mod v3;

/// One-time bring-up. Reads the discovered GIC version + register
/// windows from the HAL and brings the appropriate driver up.
pub fn init() {
    let bi = crate::hal::info();
    match bi.intc.kind {
        // SAFETY: register windows are valid identity-mapped MMIO and
        // we are the only CPU running.
        IntcKind::GicV2 => unsafe {
            v2::init(
                bi.intc.primary.base as usize,
                bi.intc.secondary.base as usize,
            );
        },
        // SAFETY: as above.
        IntcKind::GicV3 => unsafe {
            v3::init(
                bi.intc.primary.base as usize,
                bi.intc.secondary.base as usize,
            );
        },
        // We never end up here on aarch64; the HAL only ever sets these
        // for the x86_64 backend.
        IntcKind::Apic | IntcKind::Pic8259 => {
            crate::log::error!(
                "GIC bring-up requested on a non-GIC interrupt controller ({:?})",
                bi.intc.kind
            );
        }
    }
}

/// Enable a Private Peripheral Interrupt (PPI), INTID 16..=31.
pub fn enable_ppi(intid: u32) {
    debug_assert!((16..32).contains(&intid));
    match crate::hal::info().intc.kind {
        IntcKind::GicV2 => v2::enable_ppi(intid),
        IntcKind::GicV3 => v3::enable_ppi(intid),
        IntcKind::Apic | IntcKind::Pic8259 => {}
    }
}

/// IRQ trap entry. Reads the active interrupt, dispatches it, signals
/// EOI. Called from [`super::exceptions::hyperion_trap_irq`].
pub fn handle_irq() {
    let intid = match crate::hal::info().intc.kind {
        IntcKind::GicV2 => v2::ack(),
        IntcKind::GicV3 => v3::ack(),
        IntcKind::Apic | IntcKind::Pic8259 => return,
    };
    if intid == 1023 {
        // Spurious; nothing to do, no EOI to write.
        return;
    }

    if super::timer::is_timer_irq(intid) {
        super::timer::rearm();
        crate::proc::scheduler::on_tick();
    } else {
        crate::log::warn!("unrouted IRQ {}", intid);
    }

    match crate::hal::info().intc.kind {
        IntcKind::GicV2 => v2::eoi(intid),
        IntcKind::GicV3 => v3::eoi(intid),
        IntcKind::Apic | IntcKind::Pic8259 => {}
    }
}
