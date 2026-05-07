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

use crate::hal::boot_info::GicVersion;

mod v2;
mod v3;

/// One-time bring-up. Reads the discovered GIC version + register
/// windows from the HAL and brings the appropriate driver up.
pub fn init() {
    let bi = crate::hal::info();
    match bi.gic.version {
        // SAFETY: register windows are valid identity-mapped MMIO and
        // we are the only CPU running.
        GicVersion::V2 => unsafe {
            v2::init(bi.gic.dist.base as usize, bi.gic.cpu_or_redist.base as usize);
        },
        // SAFETY: as above.
        GicVersion::V3 => unsafe {
            v3::init(bi.gic.dist.base as usize, bi.gic.cpu_or_redist.base as usize);
        },
    }
}

/// Enable a Private Peripheral Interrupt (PPI), INTID 16..=31.
pub fn enable_ppi(intid: u32) {
    debug_assert!((16..32).contains(&intid));
    match crate::hal::info().gic.version {
        GicVersion::V2 => v2::enable_ppi(intid),
        GicVersion::V3 => v3::enable_ppi(intid),
    }
}

/// IRQ trap entry. Reads the active interrupt, dispatches it, signals
/// EOI. Called from [`super::exceptions::hyperion_trap_irq`].
pub fn handle_irq() {
    let intid = match crate::hal::info().gic.version {
        GicVersion::V2 => v2::ack(),
        GicVersion::V3 => v3::ack(),
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

    match crate::hal::info().gic.version {
        GicVersion::V2 => v2::eoi(intid),
        GicVersion::V3 => v3::eoi(intid),
    }
}
