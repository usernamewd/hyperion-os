//! aarch64 boot stub.
//!
//! `_start` is pinned by the linker into `.text.boot` and is the first thing
//! that executes when the kernel is loaded — be that QEMU `-kernel`,
//! U-Boot, the EFI stub, or a future Pi mailbox loader. We do the
//! minimum amount of work needed before transferring to Rust:
//!
//! * Park secondary CPUs (we boot single-core; SMP is a follow-up).
//! * Drop to EL1 if we entered at EL2 (QEMU `virt` does, U-Boot's
//!   default does, most boot firmware does).
//! * Set up SP, zero BSS, install a tiny early exception vector (so
//!   accidental traps before `late_init` halt cleanly), and call into
//!   the Rust trampoline.
//!
//! The trampoline parses the device-tree blob handed in `x0` (when
//! present) and instantiates the HAL — including the active console
//! UART — before [`crate::kmain`] runs. So the very first `log!` line
//! is already emitted via the right driver for the board we're on.
//!
//! The Linux aarch64 boot protocol image header lives at offset 0 so
//! the same kernel can be loaded as a flat binary by firmware that
//! requires it (U-Boot's `booti`, ARM's `kernel8.img`, etc.).

use core::arch::global_asm;

// `_start` is written in raw assembly so we can guarantee instruction
// ordering and avoid any compiler-inserted prologue before the stack is set
// up. It is placed in `.text.boot`, which the linker script puts at the
// start of the kernel image.
//
// The Linux aarch64 boot protocol expects an image header at offset 0; QEMU
// loading an ELF doesn't strictly require it, but we keep the layout
// compatible so the same kernel can later be loaded as a flat binary by
// firmware that wants the header.
global_asm!(
    r#"
    .section .text.boot, "ax"
    .globl _start
    .type _start, @function
_start:
    // x0 holds the DTB physical address on real hardware / U-Boot. QEMU
    // virt also passes it. We stash it in a fixed register that survives
    // until kmain is called.
    mov     x19, x0

    // ---- Park secondary CPUs ----
    // MPIDR_EL1[7:0] is Aff0 for the simple uniprocessor cluster QEMU
    // builds; non-zero => secondary => sleep forever.
    mrs     x1, mpidr_el1
    and     x1, x1, #0xff
    cbnz    x1, 2f

    // ---- Drop to EL1 if needed ----
    mrs     x0, CurrentEL
    lsr     x0, x0, #2
    cmp     x0, #2
    b.ne    1f                  // already at EL1 (or EL0/EL3)

    // Configure HCR_EL2 so EL1 runs in aarch64 with no traps.
    mov     x0, #(1 << 31)      // RW=1: EL1 is aarch64
    msr     hcr_el2, x0

    // SCTLR_EL1: caches/MMU off for now, RES1 bits set.
    mov     x0, #0x0800
    movk    x0, #0x30d0, lsl #16
    msr     sctlr_el1, x0

    // SPSR_EL2 = EL1h with DAIF masked.
    mov     x0, #0x3c5
    msr     spsr_el2, x0

    // ELR_EL2 -> the EL1 entry below.
    adr     x0, 1f
    msr     elr_el2, x0
    eret

1:  // ---- We are now at EL1, MMU off, caches off ----

    // Enable FP/SIMD at EL1/EL0 so the compiler can emit Q/D/V regs
    // (Rust does this for memset/memcpy and for any code that touches
    // 128-bit moves). CPACR_EL1.FPEN = 0b11 (no traps).
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0
    isb

    // Set up the kernel stack. __stack_top is provided by the linker.
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0

    // ---- Zero .bss ----
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
3:  cmp     x0, x1
    b.hs    4f
    str     xzr, [x0], #8
    b       3b
4:

    // Install the *early* exception vector before calling Rust. If we
    // fault before late_init runs we want to halt rather than wander into
    // the weeds.
    adrp    x0, __early_vectors
    add     x0, x0, :lo12:__early_vectors
    msr     vbar_el1, x0
    isb

    // Restore DTB pointer (saved in x19 above) and tail-call into Rust.
    // The trampoline parses the DTB, brings up the HAL (including the
    // console UART), then jumps into kmain.
    mov     x0, x19
    bl      hyperion_kernel_kmain_trampoline

    // kmain returned (it shouldn't). Fall through to halt.
2:  wfe
    b       2b

    .size _start, . - _start

    // ---- Early exception vectors: just halt ----
    .balign 0x800
__early_vectors:
    // 16 entries, each 0x80 bytes apart. We don't care which fired.
    .rept 16
    .balign 0x80
    b       2b
    .endr
"#
);

/// Trampoline called from `_start`. Exists so that `_start` can be raw
/// assembly without referencing Rust-mangled names directly.
///
/// `boot_arg` is either the physical address of a device-tree blob or a
/// pointer to the UEFI handoff block built by `efi-stub/`. Address `0`
/// means "no firmware table", in which case the HAL falls back to a
/// compile-time QEMU-virt machine description so the legacy
/// `qemu-system-aarch64 -kernel hyperion-kernel` path keeps working.
#[no_mangle]
pub extern "C" fn hyperion_kernel_kmain_trampoline(boot_arg: u64) -> ! {
    let bi = uefi_handoff(boot_arg).unwrap_or_else(|| {
        // SAFETY: this runs exactly once, with interrupts masked and no
        // other CPU awake. `boot_arg` is whatever firmware put in x0;
        // the parser validates a DTB magic before reading further.
        unsafe { crate::hal::dtb::parse_or_fallback(boot_arg) }
    });
    // SAFETY: `bi` describes mapped MMIO regions on this board; the
    // HAL initialises the active console driver here.
    unsafe { crate::hal::init(bi) };
    crate::kmain();
}

fn uefi_handoff(addr: u64) -> Option<crate::hal::BootInfo> {
    if addr == 0 {
        return None;
    }
    let handoff = unsafe { (addr as *const crate::hal::boot_info::UefiHandoff).read_unaligned() };
    handoff.to_boot_info()
}
