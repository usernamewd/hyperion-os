//! Architectural timer for x86_64.
//!
//! Wraps the TSC (for monotonic time) and the LAPIC timer (for the
//! periodic scheduler tick). The arch facade exposes [`read_freq`] and
//! [`read_count`]; everything else is consumed by [`init`] and the
//! tick handler in [`super::exceptions`].

use core::sync::atomic::{AtomicU64, Ordering};

const SCHEDULER_HZ: u64 = 100;
const TICK_VECTOR: u8 = 0x20;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Bring up the TSC (calibrated) and the LAPIC timer (periodic at
/// [`SCHEDULER_HZ`]).
pub fn init() {
    // 1. Calibrate the TSC against the PIT.
    let tsc_hz = super::pit::calibrate_tsc();
    super::tsc::set_freq(tsc_hz);
    crate::log::info!("x86: TSC frequency = {} MHz", tsc_hz / 1_000_000);

    // 2. Calibrate the LAPIC bus rate.
    let bus_hz = super::pit::calibrate_lapic();
    if bus_hz != 0 {
        let interval = (bus_hz / SCHEDULER_HZ) as u32;
        super::apic::init_timer(TICK_VECTOR, interval.max(1));
        crate::log::info!(
            "x86: LAPIC bus = {} MHz, scheduler @ {} Hz (interval = {} ticks)",
            bus_hz / 1_000_000,
            SCHEDULER_HZ,
            interval
        );
    } else {
        crate::log::warn!("x86: LAPIC bus calibration failed; scheduler tick disabled");
    }
}

/// Bus-tick callback from the LAPIC timer trap stub. Increments the
/// monotonic tick counter and lets the scheduler take a turn.
pub fn on_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Read the boot CPU's monotonic counter frequency in Hz.
pub fn read_freq() -> u64 {
    super::tsc::read_freq()
}

/// Read the boot CPU's monotonic counter value.
pub fn read_count() -> u64 {
    super::tsc::read()
}
