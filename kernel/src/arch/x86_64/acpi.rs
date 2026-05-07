//! ACPI shutdown / reset hooks.
//!
//! We don't ship a full ACPI parser yet (RSDP / FADT / DSDT / AML /
//! …), so we use the well-known shortcuts that work under QEMU and
//! every BIOS-class firmware shipped in the last 20 years:
//!
//! * **Shutdown.** QEMU's `-machine q35`/`pc` exposes the ACPI sleep
//!   register at I/O port `0x604`. Writing `0x2000` triggers a
//!   `S5` (soft-off) transition. On Bochs / VirtualBox this same
//!   trick works at port `0xB004`. Real hardware would require AML
//!   evaluation, which is out of scope.
//! * **Reset.** Writing `0xFE` to the keyboard controller port `0x64`
//!   triple-faults the CPU on every PC since the AT.

use super::{outb, outl};

const QEMU_SHUTDOWN_PORT: u16 = 0x604;
const QEMU_SHUTDOWN_PORT_BOCHS: u16 = 0xB004;
const QEMU_SHUTDOWN_PORT_VBOX: u16 = 0x4004;
const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;

const PS2_CMD: u16 = 0x64;
const RESET_CMD: u8 = 0xFE;

/// Power off the system. Falls back to halting forever if the
/// shutdown ports don't exist (e.g. real hardware without ACPI).
pub fn system_off() -> ! {
    // SAFETY: legacy I/O ports — writes are inert if the device isn't
    // wired up.
    unsafe {
        // Try the QEMU q35/pc port first, then Bochs/VBox shutdown ports.
        super::outw(QEMU_SHUTDOWN_PORT, 0x2000);
        super::outw(QEMU_SHUTDOWN_PORT_BOCHS, 0x2000);
        super::outw(QEMU_SHUTDOWN_PORT_VBOX, 0x3400);
        // ISA debug-exit (qemu `-device isa-debug-exit`): exit code 0
        // means the test harness reached a clean exit. Useful for CI.
        outl(QEMU_DEBUG_EXIT_PORT, 0x10);
    }
    super::halt();
}

/// Reset the system via the keyboard controller's CPU reset line.
pub fn system_reset() -> ! {
    // SAFETY: legacy I/O port.
    unsafe { outb(PS2_CMD, RESET_CMD) };
    super::halt();
}
