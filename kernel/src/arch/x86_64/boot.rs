//! x86_64 boot stub.
//!
//! Two entry paths land here:
//!
//! * **Multiboot2** (GRUB-PC / GRUB-EFI / `qemu -kernel` once our
//!   Multiboot2 header is recognised). GRUB enters in 32-bit protected
//!   mode at the address advertised by the *entry address* tag in the
//!   [.multiboot2_header](#) — we point it at [`_start`], which sets
//!   up paging, switches to long mode, and tail-calls
//!   [`hyperion_kernel_kmain_trampoline`] with `rdi` = the Multiboot2
//!   info pointer.
//! * **Native UEFI** (Hyperion's own EFI stub at `efi-stub/`, built
//!   for `x86_64-unknown-uefi`). The stub stays in 64-bit long mode
//!   the whole time, loads the kernel ELF, and jumps to the ELF's
//!   `e_entry`. We pin `e_entry` at [`_start_uefi`] in
//!   `linker-x86_64.ld`. `_start_uefi` switches to the kernel's own
//!   stack, then tail-calls [`hyperion_kernel_uefi_trampoline`] with
//!   `rdi` = a pointer to the [`UefiHandoff`](crate::hal::boot_info::UefiHandoff)
//!   block the stub assembled.
//!
//! `_start` and `_start_uefi` cannot share machine code because they
//! enter the CPU in different modes (32-bit protected vs 64-bit long).
//! They share *everything else* — both end in a Rust trampoline that
//! reaches `crate::kmain` via the same HAL bring-up sequence.
//!
//! GRUB picks the Multiboot2 entry-address tag over the ELF's
//! `e_entry` field, so changing `e_entry` to `_start_uefi` (for the
//! UEFI path) doesn't break GRUB's BIOS/EFI Multiboot2 path.

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

    // ---- entry-address tag (32-bit) ----
    // GRUB-PC and GRUB-EFI both honour this tag and ignore the ELF
    // header's e_entry field when it's present. We point it at the
    // 32-bit-protected-mode `_start`, so changing the ELF's e_entry to
    // the 64-bit `_start_uefi` (used by the native UEFI stub) doesn't
    // break the GRUB / qemu-multiboot2 path.
    .balign 8
    .word   3                              // type = ENTRY_ADDRESS
    .word   0                              // flags
    .long   12                             // size
    .long   _start                         // entry_addr (32-bit)

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

    // ---- Zero .bss ----
    // The boot page tables and the boot stack live inside .bss, so we
    // MUST zero it now while we're still in identity-mapped real mode.
    // Doing it after enabling paging (the obvious place) wipes out the
    // tables we're using to translate, causing an instant triple fault.
    // EDI holds the multiboot2 info pointer; save/restore around the
    // stos. ESI holds the magic and is preserved by stosd.
    push    edi
    lea     edi, [__bss_start]
    lea     ecx, [__bss_end]
    sub     ecx, edi
    shr     ecx, 2
    xor     eax, eax
    rep     stosd
    pop     edi

    // ---- Build identity page tables for the bottom 1 GiB ----
    //   PML4[0] -> PDPT
    //   PDPT[0] -> PD
    //   PD[0..512] -> 2MiB pages (PRESENT|WRITE|HUGE) covering 0..1 GiB.
    // Anything above 1 GiB (LAPIC at 0xFEE00000, framebuffers, etc.)
    // falls into the next 1 GiB and we map *that* whole gigabyte too,
    // through a second PDPT[3] entry, for free at boot.
    //
    // The three tables sit in .bss which we just zeroed, so we don't
    // need a redundant clear here.

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

    // .bss was already zeroed in 32-bit mode (zeroing it here would
    // wipe the still-active __boot_pml4/PDPT/PD that live in .bss).
    //
    // Multiboot2 info pointer is in EDI/RDI from way earlier; magic is
    // in ESI/RSI. Pass them to the trampoline. RDI is already the MB2
    // info ptr; the magic goes in RSI (which is the second sysv arg).
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

// 64-bit UEFI entry. The Hyperion EFI stub jumps here in long mode
// after ExitBootServices, with `rdi` = a pointer to the
// [`UefiHandoff`](crate::hal::boot_info::UefiHandoff) block it built
// from GOP / memory map / EFI configuration tables. The CPU is already
// in long mode on EFI x86_64, so unlike `_start` we don't have to
// build page tables, switch modes, or load a GDT — UEFI's tables stay
// usable until `gdt::install` and the kernel's own paging code run
// later in `arch::late_init`.
//
// We do switch to the kernel's own __boot_stack_top because the EFI
// stub's stack lived in EFI loader-data pages that are no longer
// owned by anyone after ExitBootServices.
global_asm!(
    r#"
    .section .text.boot, "ax"
    .globl _start_uefi
    .code64
    .type _start_uefi, @function
_start_uefi:
    cli
    cld

    // RDI = handoff pointer (sysv first arg). Stash so we don't lose
    // it across the stack swap.
    mov     rax, rdi

    // Switch to the kernel's pre-allocated boot stack. The EFI stub
    // already zeroed .bss as part of loading our PT_LOAD segments, so
    // the stack region is clean.
    lea     rsp, [__boot_stack_top]

    // Forward the handoff pointer in rdi (System V ABI first arg).
    mov     rdi, rax
    xor     rbp, rbp
    call    hyperion_kernel_uefi_trampoline

    // Trampoline shouldn't return; if it does, halt.
9:  hlt
    jmp     9b
    .size _start_uefi, . - _start_uefi
"#
);

/// Trampoline called from `_start_uefi`. Sister to
/// [`hyperion_kernel_kmain_trampoline`].
///
/// `handoff_ptr` is the physical address of a
/// [`UefiHandoff`](crate::hal::boot_info::UefiHandoff) block built by
/// the Hyperion EFI stub before `ExitBootServices`. If the magic /
/// version don't validate we fall back to
/// [`BootInfo::qemu_q35_fallback`](crate::hal::boot_info::BootInfo::qemu_q35_fallback)
/// so we still reach the shell — that path is identical to what the
/// raw `-kernel` boot would have done.
#[no_mangle]
pub extern "C" fn hyperion_kernel_uefi_trampoline(handoff_ptr: u64) -> ! {
    let bi = uefi_handoff(handoff_ptr).unwrap_or_else(|| {
        // No (or malformed) handoff: drop into the same q35 fallback
        // path Multiboot2 uses when its info block is missing.
        unsafe { crate::hal::multiboot::parse_or_fallback(0, 0) }
    });
    unsafe { super::serial::early_init() };
    unsafe { crate::hal::init(bi) };
    crate::kmain();
}

fn uefi_handoff(addr: u64) -> Option<crate::hal::BootInfo> {
    if addr == 0 {
        return None;
    }
    // SAFETY: the EFI stub guarantees the pointer is page-aligned and
    // points at a UefiHandoff structure for the duration of the kernel
    // lifetime (the page is allocated as EfiLoaderData which we keep
    // after ExitBootServices). `to_boot_info` validates magic/version.
    let handoff = unsafe { (addr as *const crate::hal::boot_info::UefiHandoff).read_unaligned() };
    handoff.to_boot_info()
}
