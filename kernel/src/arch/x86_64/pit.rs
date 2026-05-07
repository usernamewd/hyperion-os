//! 8254 Programmable Interval Timer.
//!
//! We never actually use the PIT for periodic ticks — that's the
//! LAPIC's job — but we use it twice during bring-up:
//!
//! 1. **TSC calibration.** Program the PIT in one-shot mode for a
//!    fixed interval, busy-wait by polling the channel-2 gate, and
//!    measure how many TSC ticks elapsed. From that we derive the
//!    boot CPU's TSC frequency in Hz.
//! 2. **LAPIC timer calibration.** Same thing for the LAPIC timer's
//!    bus-tick rate. Result is fed into [`super::apic::init_timer`].
//!
//! After bring-up the PIT is left disabled (no IRQs reach the CPU
//! anyway because the 8259 PIC is fully masked).

use super::{inb, outb};

const PIT_CHANNEL2_DATA: u16 = 0x42;
const PIT_CMD: u16 = 0x43;
const PORT_61: u16 = 0x61;

/// 1.193182 MHz (the canonical PIT input clock).
pub const PIT_FREQ_HZ: u32 = 1_193_182;

/// Run a calibration window of `microseconds` µs and return the value
/// returned by `read()` *after* the window minus before.
///
/// The closure is called twice — once before, once after.
fn measure<F: FnMut() -> u64>(microseconds: u32, mut read: F) -> u64 {
    let count = (PIT_FREQ_HZ as u64 * microseconds as u64) / 1_000_000;
    let count: u16 = count.min(0xFFFF) as u16;

    // SAFETY: legacy PIT / port 0x61 are documented for every PC.
    unsafe {
        // Disable the speaker line, enable the gate.
        let mut p61 = inb(PORT_61);
        p61 = (p61 & !0x02) | 0x01;
        outb(PORT_61, p61);
        // Channel 2, mode 0 (interrupt on terminal count), binary.
        outb(PIT_CMD, 0xB0);
        outb(PIT_CHANNEL2_DATA, (count & 0xFF) as u8);
        outb(PIT_CHANNEL2_DATA, (count >> 8) as u8);

        let start = read();
        // Wait until OUT2 (port 0x61, bit 5) goes high — that is the
        // PIT signalling "count expired".
        while inb(PORT_61) & 0x20 == 0 {
            core::hint::spin_loop();
        }
        let end = read();
        end.wrapping_sub(start)
    }
}

/// Calibrate the TSC frequency in Hz against the PIT.
pub fn calibrate_tsc() -> u64 {
    // 50 ms is plenty long for a stable measurement and short enough
    // to fit in a 16-bit PIT counter (max ≈ 54.9 ms at 1.193 MHz).
    let micros = 50_000;
    let delta = measure(micros, super::tsc::read);
    delta * 1_000_000 / micros as u64
}

/// Calibrate the LAPIC bus tick rate (Hz). We use the (already
/// calibrated) TSC as the time base instead of the PIT — under UEFI
/// firmware (OVMF) the PIT channel-2 gate at port 0x61 doesn't strobe
/// reliably a second time, so a PIT-based window hangs forever in
/// `measure`. The TSC has none of those problems and is also more
/// accurate.
pub fn calibrate_lapic() -> u64 {
    let bi = crate::hal::info();
    let base = bi.intc.primary.base as usize;
    if base == 0 {
        return 0;
    }

    let tsc_freq = super::tsc::read_freq();
    if tsc_freq == 0 {
        return 0;
    }
    let calib_us: u64 = 50_000;
    let tsc_ticks_to_wait = tsc_freq.saturating_mul(calib_us) / 1_000_000;

    // SAFETY: LAPIC is mapped.
    unsafe {
        // Divide by 16 (matches `apic::init_timer`).
        core::ptr::write_volatile((base + 0x3E0) as *mut u32, 0b0011);
        // Mask the timer LVT during calibration (vector 0x20, masked).
        core::ptr::write_volatile((base + 0x320) as *mut u32, (1 << 16) | 0x20);
        // Program one-shot countdown from u32::MAX.
        core::ptr::write_volatile((base + 0x380) as *mut u32, u32::MAX);

        let tsc_start = super::tsc::read();
        let lapic_start =
            u32::MAX as u64 - core::ptr::read_volatile((base + 0x390) as *const u32) as u64;
        while super::tsc::read().wrapping_sub(tsc_start) < tsc_ticks_to_wait {
            core::hint::spin_loop();
        }
        let lapic_end =
            u32::MAX as u64 - core::ptr::read_volatile((base + 0x390) as *const u32) as u64;

        // Stop the LAPIC timer (initial count = 0 disables it).
        core::ptr::write_volatile((base + 0x380) as *mut u32, 0);

        let delta = lapic_end.wrapping_sub(lapic_start);
        delta.saturating_mul(1_000_000) / calib_us
    }
}
