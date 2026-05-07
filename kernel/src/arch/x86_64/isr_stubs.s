// 256 ISR entry stubs for the x86_64 IDT.
//
// Each stub:
//   1. Pushes a fake 0 error-code (only for vectors that don't have a
//      hardware error code), so __trap_common can rely on a uniform
//      stack layout.
//   2. Pushes its vector number.
//   3. Jumps to __trap_common (defined in exceptions.rs).
//
// __isr_stub_table is a 256-entry array of u64, one per stub, used by
// install_idt() to populate the IDT.
//
// Vectors that do push an error code: 8, 10, 11, 12, 13, 14, 17, 21,
// 29, 30. All others get a fake error code from us so the unwind path
// can skip a fixed-size frame.

.section .text
.balign 16

.macro ISR_NOERR vec
    .balign 16
    .globl __isr\vec
__isr\vec:
    push    0
    push    \vec
    jmp     __trap_common
.endm

.macro ISR_ERR vec
    .balign 16
    .globl __isr\vec
__isr\vec:
    push    \vec
    jmp     __trap_common
.endm

// ---------------- Generate stubs 0..=255 ----------------
ISR_NOERR 0
ISR_NOERR 1
ISR_NOERR 2
ISR_NOERR 3
ISR_NOERR 4
ISR_NOERR 5
ISR_NOERR 6
ISR_NOERR 7
ISR_ERR   8
ISR_NOERR 9
ISR_ERR   10
ISR_ERR   11
ISR_ERR   12
ISR_ERR   13
ISR_ERR   14
ISR_NOERR 15
ISR_NOERR 16
ISR_ERR   17
ISR_NOERR 18
ISR_NOERR 19
ISR_NOERR 20
ISR_ERR   21
ISR_NOERR 22
ISR_NOERR 23
ISR_NOERR 24
ISR_NOERR 25
ISR_NOERR 26
ISR_NOERR 27
ISR_NOERR 28
ISR_ERR   29
ISR_ERR   30
ISR_NOERR 31

.irp vec,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63
ISR_NOERR \vec
.endr
.irp vec,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95
ISR_NOERR \vec
.endr
.irp vec,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127
ISR_NOERR \vec
.endr
.irp vec,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159
ISR_NOERR \vec
.endr
.irp vec,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,188,189,190,191
ISR_NOERR \vec
.endr
.irp vec,192,193,194,195,196,197,198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,223
ISR_NOERR \vec
.endr
.irp vec,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,252,253,254,255
ISR_NOERR \vec
.endr

// ---------------- Pointer table ----------------
.section .rodata
.balign 8
.globl __isr_stub_table
__isr_stub_table:
.macro PTR vec
    .quad __isr\vec
.endm

PTR 0
PTR 1
PTR 2
PTR 3
PTR 4
PTR 5
PTR 6
PTR 7
PTR 8
PTR 9
PTR 10
PTR 11
PTR 12
PTR 13
PTR 14
PTR 15
PTR 16
PTR 17
PTR 18
PTR 19
PTR 20
PTR 21
PTR 22
PTR 23
PTR 24
PTR 25
PTR 26
PTR 27
PTR 28
PTR 29
PTR 30
PTR 31

.irp vec,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63
PTR \vec
.endr
.irp vec,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95
PTR \vec
.endr
.irp vec,96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127
PTR \vec
.endr
.irp vec,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159
PTR \vec
.endr
.irp vec,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,188,189,190,191
PTR \vec
.endr
.irp vec,192,193,194,195,196,197,198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,223
PTR \vec
.endr
.irp vec,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,252,253,254,255
PTR \vec
.endr
