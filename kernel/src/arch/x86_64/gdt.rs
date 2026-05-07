//! Global Descriptor Table.
//!
//! The boot stub installs a tiny GDT (null / 64-bit code / data) just
//! to enter long mode. Once we're in Rust we install a more complete
//! one that adds a TSS — required if we ever take an exception with a
//! stack switch (we don't yet, but the entry below makes it cheap to
//! add later) — and ring-3 segments for future user-mode threads.
//!
//! The TSS itself is statically allocated; it lives in `.bss` and is
//! zeroed by the boot stub before we get here.

use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

const GDT_ENTRIES: usize = 7;
const _IST_STACK_SIZE: usize = 4096 * 4;

/// 64-bit Task State Segment. The only field we currently care about
/// is `iopb` (set to "no IO bitmap" by storing the size of the TSS).
#[repr(C, packed)]
struct Tss {
    _reserved0: u32,
    rsp: [u64; 3],
    _reserved1: u64,
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    iopb: u16,
}

#[repr(C, align(16))]
struct Gdt([u64; GDT_ENTRIES]);

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

static mut GDT: Gdt = Gdt([0; GDT_ENTRIES]);
static mut TSS: Tss = Tss {
    _reserved0: 0,
    rsp: [0; 3],
    _reserved1: 0,
    ist: [0; 7],
    _reserved2: 0,
    _reserved3: 0,
    iopb: 0,
};

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the kernel GDT and load segment registers.
pub fn install() {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // Build the descriptors. We keep the same selectors the boot stub
    // uses (0x08 = code, 0x10 = data) so far calls from the boot stub
    // remain valid until we reload CS below.
    let tss_base = core::ptr::addr_of!(TSS) as u64;
    let tss_limit = (size_of::<Tss>() - 1) as u64;

    // SAFETY: GDT is only mutated here, on the boot CPU, before any
    // other code runs.
    let g: *mut [u64; GDT_ENTRIES] = unsafe { core::ptr::addr_of_mut!(GDT.0) };
    unsafe {
        (*g)[0] = 0;
        (*g)[1] = 0x00AF_9A00_0000_FFFF; // 0x08 ring0 code 64-bit
        (*g)[2] = 0x00CF_9200_0000_FFFF; // 0x10 ring0 data
        (*g)[3] = 0x00AF_FA00_0000_FFFF; // 0x18 ring3 code 64-bit
        (*g)[4] = 0x00CF_F200_0000_FFFF; // 0x20 ring3 data
        (*g)[5] = tss_descriptor_low(tss_base, tss_limit);
        (*g)[6] = tss_descriptor_high(tss_base);
    }

    let ptr = GdtPointer {
        limit: (size_of::<Gdt>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };

    // SAFETY: lgdt + reload of segment registers, only on the boot CPU
    // before any preemption or interrupts. We use a long-jump-via-iret
    // trick to reload CS in 64-bit mode.
    unsafe {
        core::arch::asm!(
            "lgdt [{0}]",
            "push 0x08",
            "lea {1}, [rip + 2f]",
            "push {1}",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ax, 0x28",                  // TSS selector
            "ltr ax",
            in(reg) &ptr,
            out(reg) _,
            out("ax") _,
            options(nostack),
        );
    }
}

const fn tss_descriptor_low(base: u64, limit: u64) -> u64 {
    let access: u64 = 0x89;        // present, type=64-bit TSS available
    let granularity: u64 = 0;      // limit < 1 MiB, byte-granularity
    (limit & 0xFFFF)
        | ((base & 0xFFFF) << 16)
        | (((base >> 16) & 0xFF) << 32)
        | (access << 40)
        | (((limit >> 16) & 0xF) << 48)
        | (granularity << 52)
        | (((base >> 24) & 0xFF) << 56)
}

const fn tss_descriptor_high(base: u64) -> u64 {
    (base >> 32) & 0xFFFF_FFFF
}
