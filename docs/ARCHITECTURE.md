# Hyperion architecture

Hyperion is a **microkernel**: the kernel itself does only the work that
fundamentally must run privileged — CPU bring-up, MMU, scheduling, IPC,
exception handling, and a tiny set of platform drivers (UART, GIC, timer).
Everything else (filesystems, display compositor, UI, shell) is layered on
top using the same APIs that out-of-tree code can use.

The current build keeps all of those services in the same address space as
the kernel, but the boundaries are designed so that pieces can be moved out
to EL0 as user services later.

```
┌──────────────────────────────────────────────────────────────────────┐
│  Custom OS / UI shell  (your code, no_std, against `libos-api`)       │
├──────────────────────────────────────────────────────────────────────┤
│  Hyperion services      shell · ramfs · compositor · UI widgets       │
├──────────────────────────────────────────────────────────────────────┤
│  Microkernel core       sched · threads · IPC · caps · syscalls       │
├──────────────────────────────────────────────────────────────────────┤
│  Platform drivers       UART · GIC · timer · PSCI · MMU               │
├──────────────────────────────────────────────────────────────────────┤
│  aarch64 / QEMU virt                                                  │
└──────────────────────────────────────────────────────────────────────┘
```

## Boot flow

1. **`_start`** (asm in `kernel/src/arch/aarch64/boot.rs`)
   - Saves the device-tree pointer (`x0` → `x19`).
   - Parks secondary CPUs (`mpidr_el1 != 0` → `wfe` loop).
   - Drops from EL2 to EL1 if needed (sets `HCR_EL2.RW`, `SCTLR_EL1`,
     `SPSR_EL2 = 0x3c5` (EL1h, DAIF masked), `eret`).
   - Enables FP/SIMD at EL1 (`CPACR_EL1.FPEN = 0b11`). **Required** —
     Rust emits Q/D/V instructions for `memcpy`/`memset`.
   - Sets up the boot stack from the linker symbol `__stack_top`
     (64 KiB region in `.stack`).
   - Zeroes BSS (`__bss_start..__bss_end`).
   - Installs the early exception vector (`__early_vectors`) into
     `vbar_el1`.
   - Calls `hyperion_kernel_uart_early_init()`, then
     `hyperion_kernel_kmain_trampoline(dtb)`.

2. **`kmain`** (Rust entry, `kernel/src/lib.rs`)
   - Prints the boot banner.
   - Initialises the PMM (256 MiB managed) and the kernel heap (4 MiB).
   - Installs the real exception vectors, enables the GIC and the
     virtual generic timer at ~100 Hz.
   - Initialises the scheduler, IPC, FS, display.
   - Spawns the shell thread and enters `proc::scheduler::run()`.

## Memory layout

```
0x0900_0000        PL011 UART (MMIO)
0x0800_0000..      GIC distributor
0x0801_0000..      GIC CPU interface

0x4008_0000        kernel ELF load address (.text)
0x4009_x000        .rodata
0x4009_a000        .data + .bss + .stack + .heap (NOLOAD reserved)
0x400a_e000        kernel heap (4 MiB)
0x40b0_0000+       PMM-managed RAM (free for kernel allocations)
```

Kernel currently runs **identity-mapped with the MMU off**. Page tables
exist (`kernel/src/arch/aarch64/mmu.rs`, two-level 2 MiB block map covering
the first 1 GiB) but `enable()` is gated and only used by future EL0 work.

## Memory management

- **PMM** (`kernel/src/mm/pmm.rs`) — bitmap frame allocator, 4 KiB pages,
  256 MiB region. Reports total / free in KiB to the `mem` shell command.
- **Heap** (`kernel/src/mm/heap.rs`) — `linked_list_allocator::LockedHeap`
  global allocator over a 4 MiB region in `.heap`.
- **VMM** (`kernel/src/mm/vmm.rs`) — placeholder address-space type;
  becomes meaningful when EL0 lands.

## Processes & scheduling

- `Process`, `Thread`, `AddressSpace` (in `kernel/src/proc/`) — each
  thread has a saved `Context` (callee-saved aarch64 regs + `sp` + `lr`).
- `Scheduler` (`kernel/src/proc/scheduler.rs`) — round-robin, preemptive.
  Tick handler in the timer ISR sets `need_resched`; the scheduler picks
  the next runnable thread and performs a context switch via
  `__context_switch` in `kernel/src/proc/context.rs`.
- Newly spawned threads start at `__thread_trampoline` which calls the
  user-supplied `extern "C" fn(usize) -> !` entry.

## IPC

- `Capability` / `Handle` / `Rights` (`kernel/src/ipc/caps.rs`) —
  per-process capability table, `Handle` is an opaque `u32`.
- `Port` (`kernel/src/ipc/port.rs`) — bounded queue (`PORT_CAPACITY = 32`)
  of fixed-size `Message` (`tag: u64`, `words: [u64; 8]`,
  `payload: [u8; 64]`). `try_send` / `try_recv` are non-blocking;
  `recv()` yields until a message arrives. This is the building block
  for the eventual user-space service model.

## Filesystem

- `Inode` trait + `InodeRef = Arc<dyn Inode>` (`kernel/src/fs/inode.rs`).
- `ramfs` (`kernel/src/fs/ramfs.rs`) — `BTreeMap`-backed in-memory tree.
- VFS root (`kernel/src/fs/vfs.rs`) is a lazy-mounted `spin::Once<InodeRef>`
  exposing `resolve`, `open`, `create_file`, `create_dir`, `remove`, plus
  `OpenFlags` and `FsError`.
- Bootstrap (`kernel/src/fs/mod.rs`) seeds `/welcome`, `/version`,
  `/dev/`, `/tmp/`.

## Display

- `Framebuffer` (`kernel/src/display/framebuffer.rs`) — RGBA8 / BGRA8,
  heap-backed `Vec<u8>`, with `clear`, `put_pixel`, `fill_rect`,
  `stroke_rect`, `encode_rgba`.
- `Monitor` (`kernel/src/display/monitor.rs`) — wraps a `Framebuffer`
  and tags it `Physical` or `Virtual`. `register_monitor` returns a
  monotonically allocated `MonitorId`.
- `Compositor` (`kernel/src/display/compositor.rs`) — z-ordered list of
  layers; `render()` walks layers low-z first and alpha-blends each into
  the destination. Used by the `demo` shell command.

A virtual `1280x720` monitor is registered at boot (`display::init()`).

## UI building blocks

- `Canvas` (`kernel/src/ui/canvas.rs`) borrows `&mut Framebuffer` and
  exposes the same API as `libos_api::ui::CanvasOps`: `clear`, `fill_rect`,
  `stroke_rect`, `line` (Bresenham), `draw_text`.
- `Font8x8` (`kernel/src/ui/font.rs`) — embedded 95-glyph bitmap font for
  printable ASCII (0x20..=0x7E).
- `Widget` trait + `Panel` / `Label` / `Button` (`kernel/src/ui/widgets.rs`).
  Widgets are `Box<dyn Widget>`-friendly; `Panel` owns children.

## Shell

REPL on PL011 UART. Commands live in `kernel/src/shell/commands.rs`. The
table is just an array of `(name, fn(&[&str], &[String]))`, so adding a
command is a one-line edit. History is kept in a `Vec<String>` and
exposed by the `history` command.

## What's intentionally out of scope (for now)

- SMP scheduler (single boot CPU; secondaries are parked).
- Real EL0 / user-space (the dispatch path exists, switching is gated).
- Persistent FS, networking, USB/PCIe drivers.
- Production-grade security audit.
