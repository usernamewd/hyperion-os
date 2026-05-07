//! 8259 Programmable Interrupt Controller — masking only.
//!
//! Modern x86_64 firmware (BIOS/UEFI) hands the kernel a board with the
//! legacy PIC pair active. Even though Hyperion uses the LAPIC + I/O
//! APIC for IRQ delivery, we still need to remap the PIC vectors away
//! from 0..0x20 (because those collide with CPU exceptions if a
//! spurious PIC IRQ ever sneaks through during the LAPIC bring-up
//! window) and then mask every line in the PIC.

use super::{io_wait, outb};

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;

/// Remap the PIC IRQs to 0x20..0x30 (so they don't clash with CPU
/// exceptions in case of stray IRQs during LAPIC bring-up) and mask
/// every line. After this we never expect to see another IRQ from the
/// 8259s — the I/O APIC handles routing.
pub fn mask_all() {
    // SAFETY: PIC ports are well-known legacy I/O. The init sequence
    // below follows the canonical ICW1..ICW4 dance.
    unsafe {
        // ICW1: start init
        outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();
        // ICW2: vector offsets
        outb(PIC1_DATA, 0x20);
        io_wait();
        outb(PIC2_DATA, 0x28);
        io_wait();
        // ICW3: master/slave wiring (slave on IRQ2)
        outb(PIC1_DATA, 4);
        io_wait();
        outb(PIC2_DATA, 2);
        io_wait();
        // ICW4: 8086 mode
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();
        // OCW1: mask everything
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}
