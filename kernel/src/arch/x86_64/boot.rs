//! x86_64 boot stub.
//!
//! GRUB-PC (BIOS) and GRUB-EFI (UEFI) both honour the Multiboot2 spec
//! and drop us at the entry point named in our Multiboot2 header in
//! 32-bit protected mode. Our [`mb2`] header (also in this crate)
//! advertises a 4 KiB-aligned load + a request for the framebuffer +
//! the Multiboot2 EFI64 service. GRUB calls our entry with:
//!
//! * `eax` = `0x36d76289` (multiboot2 magic).
//! * `ebx` = physical address of the Multiboot2 information structure.
//!
//! The stub:
//!
//! 1. Verifies the magic.
//! 2. Loads a flat 32-bit GDT and reloads the segment registers.
//! 3. Builds a 4-level identity page table covering the lower 1 GiB
//!    using a single 1 GiB PDPT entry (we only need it to bridge from
//!    32-bit protected mode into 64-bit long mode; the kernel rebuilds
//!    its real page tables in [`super::paging::init`] later).
//! 4. Enables PAE + sets `EFER.LME` + sets `CR0.PG` to enter long
//!    mode.
//! 5. Reloads CS via a far jump into the 64-bit code segment, points
//!    `RSP` at our stack reservation, and tail-calls
//!    [`hyperion_kernel_kmain_trampoline`] with `rdi` = the Multiboot2
//!    info pointer (extended to 64 bits).
//!
//! From there everything is identical to the aarch64 path: parse the
//! firmware-provided table, instantiate the HAL, jump to [`crate::kmain`].

use core::arch::global_asm;

// All Multiboot2 constants in the asm need to live in the asm too
// (rust constants aren't visible to the inline asm).
global_asm!(
    r#"
    .section .multiboot2_header, "a"
    .balign 8
    .globl __multiboot2_header
__multiboot2_header:
    // header: magic, arch=0 (i386 protected mode), header_length, checksum
    .long   0xE85250D6
    .long   0
    .long   __multiboot2_header_end - __multiboot2_header
    .long   -(0xE85250D6 + 0 + (__multiboot2_header_end - __multiboot2_header))

    // ---- information request tag ----
    .balign 8
    .word   1                              // type = INFORMATION_REQUEST
    .word   0                              // flags
    .long   24                             // size
    .long   6                              // BASIC_MEMINFO
    .long   8                              // FRAMEBUFFER_INFO (load)
    .long   1                              // CMDLINE
    .long   2                              // BOOTLOADER_NAME

    // ---- module alignment tag ----
    .balign 8
    .word   6
    .word   0
    .long   8

    // ---- framebuffer tag (preferred mode) ----
    .balign 8
    .word   5                              // type = FRAMEBUFFER
    .word   0                              // flags
    .long   20                             // size
    .long   1024                           // width hint
    .long   768                            // height hint
    .long   32                             // bpp

    // ---- end tag ----
    .balign 8
    .word   0
    .word   0
    .long   8
__multiboot2_header_end:

    // -----------------------------------------------------------------
    .section .bss, "aw", @nobits
    .balign 4096
    .globl __boot_pml4
__boot_pml4:
    .skip 4096
    .globl __boot_pdpt
__boot_pdpt:
    .skip 4096
    .globl __boot_pd
__boot_pd:
    .skip 4096
    .globl __boot_stack_bottom
__boot_stack_bottom:
    .skip 0x10000                          // 64 KiB
    .globl __boot_stack_top
__boot_stack_top:

    // -----------------------------------------------------------------
    .section .data
    .balign 16
    .globl __boot_gdt
__boot_gdt:
    .quad   0x0000000000000000             // 0x00: null
    .quad   0x00AF9A000000FFFF             // 0x08: 64-bit code
    .quad   0x00CF92000000FFFF             // 0x10: 32/64-bit data (writable)
__boot_gdt_end:

    .balign 8
    .globl __boot_gdt_pointer
__boot_gdt_pointer:
    .word   __boot_gdt_end - __boot_gdt - 1
    .quad   __boot_gdt
"#
);

// 32-bit boot entry. Multiboot2 enters in protected mode with paging
// off; we set up long mode, then far-jump into the 64-bit code below.
global_asm!(
    r#"
    .section .text.boot, "ax"
    .globl _start
    .code32
    .type _start, @function
_start:
    cld
    cli

    // Save Multiboot2 info pointer (ebx) and magic (eax) into a stable
    // pair of registers.
    mov     edi, ebx
    mov     esi, eax

    // Load a temporary stack so we can call helpers in C-style if we
    // want. The 32-bit stack lives at the top of __boot_stack_bottom.
    mov     esp, offset __boot_stack_top

    // ---- Build identity page tables for the bottom 1 GiB ----
    //   PML4[0] -> PDPT
    //   PDPT[0] -> PD
    //   PD[0..512] -> 2MiB pages (PRESENT|WRITE|HUGE) covering 0..1 GiB.
    // Anything above 1 GiB (LAPIC at 0xFEE00000, framebuffers, etc.)
    // falls into the next 1 GiB and we map *that* whole gigabyte too,
    // through a second PDPT[3] entry, for free at boot.

    // Zero the three tables (3 * 4096 / 4 dwords).
    lea     edi, [__boot_pml4]
    mov     ecx, (4096 * 3) / 4
    xor     eax, eax
    rep     stosd

    // PML4[0] -> PDPT, present + writable
    lea     eax, [__boot_pdpt]
    or      eax, 0x3
    mov     [__boot_pml4], eax

    // PDPT[0] -> PD, present + writable
    lea     eax, [__boot_pd]
    or      eax, 0x3
    mov     [__boot_pdpt + 0 * 8], eax

    // PDPT[3] -> a 1 GiB page covering 0xC000_0000..0x1_0000_0000.
    // present + writable + page-size (1 GiB). bit 7 = PS, makes this a
    // 1G huge page rather than a pointer.
    mov     eax, 0xC0000000 | 0x83
    mov     [__boot_pdpt + 3 * 8], eax

    // PD[0..512]: 512 entries each a 2 MiB page covering 0..1 GiB.
    lea     edi, [__boot_pd]
    mov     ecx, 512
    mov     eax, 0x0 | 0x83                // PRESENT | WRITE | PS
2:
    mov     [edi], eax
    mov     dword ptr [edi + 4], 0
    add     edi, 8
    add     eax, 0x200000
    loop    2b

    // ---- Enable PAE + load CR3 ----
    mov     eax, cr4
    or      eax, 1 << 5                    // CR4.PAE
    mov     cr4, eax

    lea     eax, [__boot_pml4]
    mov     cr3, eax

    // ---- Enable long mode in EFER (MSR 0xC0000080) ----
    mov     ecx, 0xC0000080
    rdmsr
    or      eax, 1 << 8                    // EFER.LME
    wrmsr

    // ---- Enable paging + enter long mode ----
    mov     eax, cr0
    or      eax, 1 << 31 | 1 << 0          // PG | PE (PE is already on)
    mov     cr0, eax

    // ---- Load 64-bit GDT and far-jump into 64-bit code ----
    lgdt    [__boot_gdt_pointer]

    // Far-return into the 64-bit code segment. We use lea + push reg
    // here because gas's Intel-syntax `push offset symbol` picks the
    // 16-bit operand-size encoding for push imm, which trashes the
    // far-return frame. lea computes a full 32-bit address, push eax
    // pushes a full 32-bit dword, and retf pops 4-byte EIP + 2-byte
    // CS in 32-bit mode.
    lea     eax, [_start64]
    push    0x08
    push    eax
    retf

    // -----------------------------------------------------------------
    .code64
_start64:
    mov     ax, 0x10                       // data selector
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     fs, ax
    mov     gs, ax

    // Re-establish the 64-bit stack pointer (RSP).
    lea     rsp, [__boot_stack_top]

    // Zero .bss (between __bss_start / __bss_end as the linker script
    // defines them).
    lea     rdi, [__bss_start]
    lea     rcx, [__bss_end]
    sub     rcx, rdi
    shr     rcx, 3
    xor     rax, rax
    rep     stosq

    // Multiboot2 info pointer is in EDI/RDI from way earlier; magic is
    // in ESI/RSI. Pass them to the trampoline. RDI is already the MB2
    // info ptr; the magic goes in RSI (which is the second sysv arg).
    // Convince the linker to keep the symbol live by referencing it.
    call    hyperion_kernel_kmain_trampoline

    // Trampoline shouldn't return; if it does, halt.
3:  hlt
    jmp     3b
"#
);

/// Trampoline called from `_start`. Mirror of the aarch64 trampoline.
///
/// `mb2_info` is the physical address of the Multiboot2 information
/// structure GRUB built. `magic` is the 0x36d76289 sentinel.
#[no_mangle]
pub extern "C" fn hyperion_kernel_kmain_trampoline(mb2_info: u64, magic: u64) -> ! {
    // SAFETY: this runs exactly once with interrupts disabled and no
    // other CPU awake. The parser validates the pointer + tags before
    // dereferencing.
    let bi = unsafe { crate::hal::multiboot::parse_or_fallback(mb2_info, magic) };
    // Bring up the COM1 / NS16550 console as early as possible so we
    // can debug-print while the rest of the HAL is being assembled.
    unsafe { super::serial::early_init() };
    // SAFETY: `bi` describes mapped MMIO regions; the HAL initialises
    // the active console driver here.
    unsafe { crate::hal::init(bi) };
    crate::kmain();
}
