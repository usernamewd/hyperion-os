//! I/O APIC driver — minimal.
//!
//! Hyperion uses the I/O APIC to route legacy ISA / PCI IRQs to LAPIC
//! vectors. Right now we just need it for the COM1 serial input
//! interrupt (IRQ4 -> vector 0x24); the rest stay masked until a
//! driver requests them. The LAPIC timer comes from the LAPIC itself,
//! not the I/O APIC.

use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

const IOREGSEL: usize = 0x00;
const IOWIN: usize = 0x10;

const IOAPICVER: u32 = 0x01;

const IOREDTBL_BASE: u32 = 0x10;

const REDTBL_MASKED: u64 = 1 << 16;

static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0);

unsafe fn read(base: u64, reg: u32) -> u32 {
    // SAFETY: caller guarantees `base` is the mapped I/O APIC.
    unsafe {
        ptr::write_volatile((base as usize + IOREGSEL) as *mut u32, reg);
        ptr::read_volatile((base as usize + IOWIN) as *const u32)
    }
}

unsafe fn write(base: u64, reg: u32, val: u32) {
    // SAFETY: caller guarantees `base` is the mapped I/O APIC.
    unsafe {
        ptr::write_volatile((base as usize + IOREGSEL) as *mut u32, reg);
        ptr::write_volatile((base as usize + IOWIN) as *mut u32, val);
    }
}

unsafe fn write_redirect(base: u64, gsi: u32, value: u64) {
    // SAFETY: caller guarantees `base` is the mapped I/O APIC and
    // `gsi` is in range.
    let lo = (value & 0xFFFF_FFFF) as u32;
    let hi = (value >> 32) as u32;
    unsafe {
        write(base, IOREDTBL_BASE + gsi * 2, lo);
        write(base, IOREDTBL_BASE + gsi * 2 + 1, hi);
    }
}

/// Bring up the I/O APIC — read its version, mask every redirection
/// entry. Drivers call [`route_irq`] to selectively unmask lines.
pub fn init() {
    let bi = crate::hal::info();
    let base = bi.intc.secondary.base;
    if base == 0 {
        return;
    }
    IOAPIC_BASE.store(base, Ordering::Release);

    // SAFETY: BootInfo asserts the I/O APIC MMIO is mapped.
    unsafe {
        let ver = read(base, IOAPICVER);
        let n_lines = ((ver >> 16) & 0xFF) + 1;
        for i in 0..n_lines {
            write_redirect(base, i, REDTBL_MASKED);
        }
    }
}

/// Route an ISA / PCI IRQ line to a LAPIC vector. `vector` should be in
/// the 0x20..0xFF range (we own 0x20..0xEF as the IRQ pool).
#[allow(dead_code)]
pub fn route_irq(gsi: u32, vector: u8) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let lapic_id: u64 = (super::apic::id() as u64) << 56;
    // Edge-triggered, fixed delivery, physical destination, unmasked.
    let entry: u64 = (vector as u64) | lapic_id;
    // SAFETY: I/O APIC is mapped.
    unsafe { write_redirect(base, gsi, entry) };
}
