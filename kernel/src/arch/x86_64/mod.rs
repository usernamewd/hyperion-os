//! x86_64 (amd64) architecture support.
//!
//! Hyperion targets long-mode amd64 launched via the Multiboot2 boot
//! protocol. The supported entry paths are:
//!
//! * **Legacy BIOS** — GRUB-PC chainloads the kernel as a Multiboot2
//!   payload from a hybrid ISO assembled by `scripts/build-iso-x86_64.sh`.
//! * **UEFI** — GRUB-EFI on the same ISO loads the kernel via
//!   Multiboot2's `EFI64` tag and passes through the UEFI image handle
//!   so we can later shut down via `RT->ResetSystem`.
//!
//! In both cases GRUB drops us at [`boot::start_x86_64`] in 32-bit
//! protected mode (or 64-bit on UEFI when GRUB has the
//! `multiboot2.cfg` `set efi_arch=x64` line). The boot stub builds a
//! minimal long-mode page table, switches to 64-bit, zeros BSS, and
//! tail-calls [`boot::hyperion_kernel_kmain_trampoline`] — which is
//! the canonical x86 mirror of the aarch64 `boot::hyperion_kernel_kmain_trampoline`.

pub mod acpi;
pub mod apic;
pub mod boot;
pub mod context;
pub mod exceptions;
pub mod gdt;
pub mod ioapic;
pub mod paging;
pub mod pic;
pub mod pit;
pub mod serial;
pub mod timer;
pub mod tsc;

/// Late architecture initialisation, called from [`crate::kmain`] after
/// the memory subsystem is online.
pub fn late_init() {
    gdt::install();
    exceptions::install_idt();
    pic::mask_all();
    apic::init();
    ioapic::init();
    timer::init();
}

/// Halt the current CPU forever. Used as the final stop in panic /
/// shutdown paths.
pub fn halt() -> ! {
    // SAFETY: cli is always safe in ring 0; outside ring 0 it would
    // #GP, but the kernel never runs in user mode through this path.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    loop {
        // SAFETY: hlt halts the CPU until the next interrupt; with
        // interrupts disabled the loop is effectively a permanent stall.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// Read a byte from an x86 I/O port.
///
/// # Safety
/// Caller must ensure the port is mapped to a device that tolerates
/// the read (i.e. it isn't sensitive to side effects).
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    // SAFETY: port-mapped I/O is always available in ring 0.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// Write a byte to an x86 I/O port.
///
/// # Safety
/// See [`inb`].
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    // SAFETY: port-mapped I/O is always available in ring 0.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

/// Read a 32-bit value from an x86 I/O port.
///
/// # Safety
/// See [`inb`].
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let v: u32;
    // SAFETY: port-mapped I/O is always available in ring 0.
    unsafe {
        core::arch::asm!("in eax, dx", in("dx") port, out("eax") v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// Write a 32-bit value to an x86 I/O port.
///
/// # Safety
/// See [`inb`].
#[inline]
pub unsafe fn outl(port: u16, value: u32) {
    // SAFETY: port-mapped I/O is always available in ring 0.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

/// Write a 16-bit value to an x86 I/O port.
///
/// # Safety
/// See [`inb`].
#[inline]
pub unsafe fn outw(port: u16, value: u16) {
    // SAFETY: port-mapped I/O is always available in ring 0.
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
    }
}

/// Brief delay used after writing to legacy I/O ports — for instance
/// the 8259 PIC requires a small pause between command writes.
#[inline]
pub fn io_wait() {
    // The conventional trick is to write to the unused port 0x80 (BIOS
    // POST diagnostic). It is not connected to anything on modern
    // hardware but the bus cycle still gives us the delay we need.
    // SAFETY: 0x80 is documented unused on every PC since the AT.
    unsafe { outb(0x80, 0) }
}
