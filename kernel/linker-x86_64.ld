/* Hyperion OS kernel linker script for x86_64 (Multiboot2 / GRUB).
 *
 * GRUB-PC (BIOS) and GRUB-EFI (UEFI) both load Multiboot2 ELF kernels.
 * Both load the kernel at the physical address recorded in our PT_LOAD
 * program headers. We pin the kernel at 0x100000 (1 MiB), which is the
 * conventional 32-bit "high memory" slot — well above the BIOS data
 * area / EBDA and out of the way of both GRUB's own image and any
 * legacy <1 MiB MMIO windows.
 *
 * The Multiboot2 header is included in the .text segment so it lives
 * within the first 32 KiB of the file (which is the spec requirement
 * for GRUB to find it).
 */

ENTRY(_start)

KERNEL_LOAD_ADDR = 0x00100000;

PHDRS
{
    text PT_LOAD FLAGS(5);    /* R-X */
    rodata PT_LOAD FLAGS(4);  /* R-- */
    data PT_LOAD FLAGS(6);    /* RW- */
}

SECTIONS
{
    . = KERNEL_LOAD_ADDR;
    PROVIDE(__kernel_start = .);

    .text : ALIGN(4K) {
        /* The Multiboot2 header MUST be in the first 32768 bytes of
         * the kernel image. Putting it first inside .text guarantees
         * that. */
        KEEP(*(.multiboot2_header))
        KEEP(*(.text.boot))
        *(.text .text.*)
    } :text

    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } :rodata

    .data : ALIGN(4K) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } :data

    .bss (NOLOAD) : ALIGN(4K) {
        PROVIDE(__bss_start = .);
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        . = ALIGN(16);
        PROVIDE(__bss_end = .);
    } :data

    /* 64 KiB initial boot stack, 16-byte aligned. The boot stub uses
     * its own .bss-allocated stack (__boot_stack_top) instead of this
     * one, but we keep the symbol around so the per-arch dispatch
     * logic can refer to it uniformly. */
    .stack (NOLOAD) : ALIGN(4K) {
        PROVIDE(__stack_bottom = .);
        . += 64K;
        PROVIDE(__stack_top = .);
    } :data

    /* 4 MiB heap region reserved at link time; managed by the kernel
     * heap allocator at runtime. */
    .heap (NOLOAD) : ALIGN(4K) {
        PROVIDE(__heap_start = .);
        . += 4M;
        PROVIDE(__heap_end = .);
    } :data

    PROVIDE(__kernel_end = .);

    /DISCARD/ : {
        *(.comment)
        *(.note .note.*)
        *(.eh_frame .eh_frame.*)
        *(.eh_frame_hdr)
    }
}
