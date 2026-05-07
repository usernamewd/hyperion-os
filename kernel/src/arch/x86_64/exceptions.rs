//! x86_64 IDT + trap dispatch.
//!
//! We install a 256-entry IDT and route every vector through one of
//! three Rust handlers:
//!
//! * `hyperion_x86_trap_exception` — vectors 0x00..0x20 (CPU faults),
//!   plus vector 0x80 reused as the syscall instruction (`int 0x80`).
//! * `hyperion_x86_trap_irq` — vectors 0x20..0xF0 (LAPIC-routed
//!   external interrupts).
//! * `hyperion_x86_trap_spurious` — vector 0xFF (LAPIC spurious).

use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};

/// CPU register state captured on exception entry. Layout matches the
/// stubs below; the [`Self::regs`] array is indexed by the
/// `Reg::*` constants.
#[repr(C)]
#[derive(Debug)]
pub struct TrapFrame {
    /// Caller-saved + callee-saved general-purpose registers, ordered
    /// in the same sequence the stubs push them onto the stack: rax,
    /// rbx, rcx, rdx, rsi, rdi, rbp, r8, r9, r10, r11, r12, r13, r14,
    /// r15.
    pub regs: [u64; 15],
    pub vector: u64,
    pub error_code: u64,
    /// Pushed by the CPU on every interrupt entry.
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Indices into [`TrapFrame::regs`] for each general-purpose register.
#[allow(dead_code)]
pub mod reg {
    pub const RAX: usize = 0;
    pub const RBX: usize = 1;
    pub const RCX: usize = 2;
    pub const RDX: usize = 3;
    pub const RSI: usize = 4;
    pub const RDI: usize = 5;
    pub const RBP: usize = 6;
    pub const R8: usize = 7;
    pub const R9: usize = 8;
    pub const R10: usize = 9;
    pub const R11: usize = 10;
    pub const R12: usize = 11;
    pub const R13: usize = 12;
    pub const R14: usize = 13;
    pub const R15: usize = 14;
}

#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    _zero: u32,
}

impl IdtEntry {
    const fn empty() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            _zero: 0,
        }
    }

    fn set(&mut self, handler: u64, type_attr: u8) {
        self.offset_low = (handler & 0xFFFF) as u16;
        self.selector = 0x08; // ring 0 code segment selector from gdt::install()
        self.ist = 0;
        self.type_attr = type_attr;
        self.offset_mid = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = ((handler >> 32) & 0xFFFF_FFFF) as u32;
        self._zero = 0;
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

const IDT_ENTRIES: usize = 256;
static mut IDT: [IdtEntry; IDT_ENTRIES] = [const { IdtEntry::empty() }; IDT_ENTRIES];
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install all 256 IDT vectors.
pub fn install_idt() {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    extern "C" {
        static __isr_stub_table: [u64; 256];
    }

    // SAFETY: IDT is only mutated here, on the boot CPU, with
    // interrupts masked.
    unsafe {
        let table_ptr: *const [u64; 256] = core::ptr::addr_of!(__isr_stub_table);
        let idt_ptr: *mut [IdtEntry; IDT_ENTRIES] = core::ptr::addr_of_mut!(IDT);
        for i in 0..IDT_ENTRIES {
            let handler = (*table_ptr)[i];
            // 0x8E = present | DPL=0 | type=interrupt gate.
            // 0xEE = present | DPL=3 | type=interrupt gate (vector 0x80).
            let attr = if i == 0x80 { 0xEE } else { 0x8E };
            (*idt_ptr)[i].set(handler, attr);
        }
    }

    let ptr = IdtPointer {
        limit: (size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };

    // SAFETY: lidt is privileged but always safe in ring 0.
    unsafe {
        core::arch::asm!("lidt [{0}]", in(reg) &ptr, options(readonly, nostack));
    }
}

/// Unmask interrupts (RFLAGS.IF = 1).
#[inline]
pub fn enable_irqs() {
    // SAFETY: sti is privileged but always safe in ring 0.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Mask interrupts (RFLAGS.IF = 0).
#[inline]
pub fn disable_irqs() {
    // SAFETY: cli is privileged but always safe in ring 0.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// CPU exception names (vectors 0..32).
const EXCEPTION_NAMES: [&str; 32] = [
    "#DE divide-by-zero",
    "#DB debug",
    "NMI",
    "#BP breakpoint",
    "#OF overflow",
    "#BR bound range exceeded",
    "#UD invalid opcode",
    "#NM device not available",
    "#DF double fault",
    "coprocessor segment overrun",
    "#TS invalid TSS",
    "#NP segment not present",
    "#SS stack-segment fault",
    "#GP general protection",
    "#PF page fault",
    "reserved (15)",
    "#MF x87 FPE",
    "#AC alignment check",
    "#MC machine check",
    "#XM SIMD FPE",
    "#VE virtualization",
    "#CP control protection",
    "reserved (22)",
    "reserved (23)",
    "reserved (24)",
    "reserved (25)",
    "reserved (26)",
    "reserved (27)",
    "#HV hypervisor injection",
    "#VC VMM communication",
    "#SX security",
    "reserved (31)",
];

/// Trap handler for CPU exceptions and `int 0x80` syscalls.
#[no_mangle]
extern "C" fn hyperion_x86_trap_exception(tf: &mut TrapFrame) {
    match tf.vector {
        0x80 => {
            // Linux-style syscall ABI on x86_64:
            //   rax = nr; args = rdi, rsi, rdx, r10, r8, r9
            let nr = tf.regs[reg::RAX];
            let args = [
                tf.regs[reg::RDI],
                tf.regs[reg::RSI],
                tf.regs[reg::RDX],
                tf.regs[reg::R10],
                tf.regs[reg::R8],
                tf.regs[reg::R9],
            ];
            let ret = crate::syscall::dispatch(nr, args);
            tf.regs[reg::RAX] = ret as u64;
        }
        v if v < 32 => panic!(
            "{} (vector {}): RIP={:#x} RSP={:#x} CS={:#x} ERR={:#x} CR2={:#x}",
            EXCEPTION_NAMES[v as usize],
            v,
            tf.rip,
            tf.rsp,
            tf.cs,
            tf.error_code,
            read_cr2(),
        ),
        v => panic!("unknown trap vector {:#x}: RIP={:#x}", v, tf.rip),
    }
}

/// Trap handler for hardware IRQs (vectors 0x20..0xFF except 0xFF).
#[no_mangle]
extern "C" fn hyperion_x86_trap_irq(tf: &mut TrapFrame) {
    let v = tf.vector as u8;
    match v {
        // LAPIC timer is wired to vector 0x20 by `apic::init_timer`.
        0x20 => {
            super::timer::on_tick();
            crate::proc::scheduler::on_tick();
        }
        // The rest are unrouted right now.
        _ => {
            crate::log::warn!("unrouted x86 IRQ vector {:#x}", v);
        }
    }
    super::apic::eoi();
}

/// LAPIC spurious interrupt vector — must NOT EOI.
#[no_mangle]
extern "C" fn hyperion_x86_trap_spurious(_tf: &mut TrapFrame) {}

#[inline]
fn read_cr2() -> u64 {
    let v: u64;
    // SAFETY: reading CR2 is privileged but always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {0}, cr2", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

// -------------------------------------------------------------------
// Trap entry stubs.
//
// One stub per vector. They build a uniform TrapFrame and tail-call the
// right Rust handler. We use a small macro so the table reads cleanly.
// -------------------------------------------------------------------

core::arch::global_asm!(
    r#"
    .section .text
    .balign 16

    // -------------------------------------------------------------
    //  trap_common: the common dispatch fragment shared by every stub.
    //  Stubs jump here after pushing (vector, error_code) onto the stack.
    //  Stack layout entering trap_common (offsets from rsp at entry):
    //
    //    rsp+0   : vector
    //    rsp+8   : error_code
    //    rsp+16  : rip pushed by CPU
    //    rsp+24  : cs
    //    rsp+32  : rflags
    //    rsp+40  : rsp pushed by CPU (always; long mode pushes it)
    //    rsp+48  : ss
    //
    //  After we save the GP regs, we have a struct TrapFrame at the top
    //  of the kernel stack and we pass its address to the handler.
    // -------------------------------------------------------------
    .balign 16
__trap_common:
    push    r15
    push    r14
    push    r13
    push    r12
    push    r11
    push    r10
    push    r9
    push    r8
    push    rbp
    push    rdi
    push    rsi
    push    rdx
    push    rcx
    push    rbx
    push    rax

    // Decide which handler to call based on the saved vector.
    // Vectors < 0x20 and vector 0x80 -> exception handler.
    // Vector == 0xFF -> spurious.
    // Otherwise -> IRQ handler.
    mov     rax, [rsp + 15 * 8]              // vector
    cmp     rax, 0xFF
    je      __trap_dispatch_spurious
    cmp     rax, 0x80
    je      __trap_dispatch_exception
    cmp     rax, 0x20
    jb      __trap_dispatch_exception

__trap_dispatch_irq:
    mov     rdi, rsp
    call    hyperion_x86_trap_irq
    jmp     __trap_return

__trap_dispatch_exception:
    mov     rdi, rsp
    call    hyperion_x86_trap_exception
    jmp     __trap_return

__trap_dispatch_spurious:
    mov     rdi, rsp
    call    hyperion_x86_trap_spurious
    // fallthrough

__trap_return:
    pop     rax
    pop     rbx
    pop     rcx
    pop     rdx
    pop     rsi
    pop     rdi
    pop     rbp
    pop     r8
    pop     r9
    pop     r10
    pop     r11
    pop     r12
    pop     r13
    pop     r14
    pop     r15
    add     rsp, 16                          // discard vector + error_code
    iretq
"#
);

// Generate the 256 stubs and the 256-entry pointer table they're looked
// up by. We generate two flavours:
//
//   * vectors 0x08, 0x0A..=0x0E, 0x11, 0x15, 0x1D, 0x1E (and the
//     standard error-pushing exceptions) — the CPU pushes an error
//     code, so we just push the vector after.
//   * everything else — we push a fake 0 error code to keep the
//     TrapFrame layout uniform.
//
// The table __isr_stub_table is declared `extern "C"` and read by
// `install_idt` above.
core::arch::global_asm!(include_str!("isr_stubs.s"));
