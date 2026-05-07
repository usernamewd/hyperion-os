//! Hardware Abstraction Layer.
//!
//! The HAL is the *only* part of the kernel that knows board-specific
//! addresses, register layouts, or boot-time conventions. Everything else
//! (mm, proc, ipc, fs, display, ui, shell) is platform-agnostic.
//!
//! Layout:
//!
//! * [`boot_info`] — the [`BootInfo`] struct that the boot stub assembles
//!   and hands to [`crate::kmain`]. Whatever managed to wake us up
//!   (legacy `-kernel` + DTB, UEFI + GOP, future bare-metal Pi mailbox,
//!   …) all funnel into this one struct.
//! * [`dtb`] — minimal Flattened Device Tree parser. Used on the legacy
//!   boot path to discover the console UART, RAM banks, GIC, timer
//!   frequency, and framebuffer.
//!
//! Drivers themselves live under [`crate::drivers`]; the HAL just decides
//! which ones to instantiate.

pub mod boot_info;
pub mod console;
pub mod dtb;

pub use boot_info::{BootInfo, ConsoleKind, GicVersion, MemoryRegion, RegSpec};

use core::sync::atomic::{AtomicBool, Ordering};

static BOOT_INFO: spin::Once<BootInfo> = spin::Once::new();
static CONSOLE_READY: AtomicBool = AtomicBool::new(false);

/// Bring up the early console from a [`BootInfo`] handed in by the boot
/// stub, then publish the [`BootInfo`] for the rest of the kernel to
/// query via [`info`].
///
/// Called exactly once, very early — before [`crate::log::init`].
///
/// # Safety
/// `bi.console.regs.base` must be a valid, identity-mapped MMIO base for
/// `bi.console.kind`. All the addresses in `bi` will be trusted by the
/// rest of the kernel without further validation.
pub unsafe fn init(bi: BootInfo) {
    // SAFETY: caller asserts the console MMIO is mapped.
    unsafe { console::install(bi.console) };
    CONSOLE_READY.store(true, Ordering::Release);
    BOOT_INFO.call_once(|| bi);
}

/// Whether the early console has been installed yet. Useful for the
/// panic handler to decide whether to attempt printing.
pub fn console_ready() -> bool {
    CONSOLE_READY.load(Ordering::Acquire)
}

/// Get the published [`BootInfo`]. Panics if the boot stub never called
/// [`init`] — which would mean the kernel is in an unrecoverable state.
pub fn info() -> &'static BootInfo {
    BOOT_INFO
        .get()
        .expect("hal::info() called before hal::init()")
}
