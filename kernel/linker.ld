/* Hyperion OS kernel linker script for aarch64 (QEMU virt machine).
 *
 * QEMU loads `-kernel <elf>` at the load address specified in the program
 * headers. We pin the kernel at 0x40080000, which is the conventional
 * location used by the aarch64 Linux boot protocol on QEMU virt
 * (RAM starts at 0x40000000; the first 0x80000 bytes are reserved
 * for boot stubs / device tree).
 */

ENTRY(_start)

KERNEL_LOAD_ADDR = 0x40080000;

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

    /* 64 KiB initial boot stack, 16-byte aligned. */
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
        *(.ARM.exidx .ARM.exidx.*)
        *(.ARM.attributes)
    }
}
