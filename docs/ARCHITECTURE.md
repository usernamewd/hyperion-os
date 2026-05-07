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
│  HAL                    BootInfo · DTB parser · Console trait         │
├──────────────────────────────────────────────────────────────────────┤
│  aarch64                QEMU virt · Pi 4/5 (UEFI) · Ampere · NXP …    │
└──────────────────────────────────────────────────────────────────────┘
```

## HAL: the portability seam

`kernel/src/hal/` is the boundary between the platform and everything
else. It exposes:

- `BootInfo` (`hal::boot_info`) — frozen, read-only struct describing
  the running platform (console, GIC, timer, RAM map, optional
  firmware framebuffer, optional DTB pointer).
- `dtb::parse_or_fallback(addr)` — minimal Flattened Device Tree (FDT
  v17) parser that walks `/memory`, `/chosen`, `/intc`, `/timer`, the
  selected UART node and any `simple-framebuffer` chosen node. Falls
  back to `BootInfo::qemu_virt_fallback()` when no DTB is provided.
- `console::BootConsole` — sum type over the supported UART drivers
  (`Pl011`, `Ns16550`, `BcmMiniUart`). Picked by the DTB parser based
  on the UART's `compatible` string.

Adding a new board reduces to teaching the DTB parser to recognise its
`compatible` strings, or — for non-DT firmware — populating
`BootInfo` directly and calling `hal::init`. See
[`PORTING.md`](./PORTING.md).

## Boot flow

There are three supported entry paths; all of them converge on the
same `kmain` after `hal::init(BootInfo)` has been called.

### Path A — direct `-kernel` (QEMU shortcut)

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
   - Calls `hyperion_kernel_kmain_trampoline(dtb)`.

2. **`kmain_trampoline`** (Rust)
   - Calls `hal::dtb::parse_or_fallback(dtb)` → `BootInfo`. If `dtb` is
     null or unparseable, returns the QEMU virt defaults.
   - `hal::init(bi)`.
   - Brings up the boot console driver picked by `hal`.
   - Tail-calls `kmain()`.

3. **`kmain`** (`kernel/src/lib.rs`)
   - Prints the boot banner.
   - Initialises the PMM from `hal::info().memory` and the kernel heap.
   - Installs the real exception vectors; brings up the GIC (v2 or v3
     depending on `hal::info().gic.version`) and the virtual generic
     timer at ~100 Hz.
   - `display::init()` — registers monitor #0. If
     `hal::info().framebuffer` is `Some`, it is wrapped as an
     `Mmio`-backed `Framebuffer` and registered as a *physical*
     monitor; otherwise a `1280x720` virtual heap-backed monitor is
     used.
   - Initialises scheduler, IPC, FS.
   - Spawns the shell thread and enters `proc::scheduler::run()`.

### Path B — DTB-described firmware boot

Same as Path A, but the bootloader (U-Boot, custom loader, …) loads
the kernel ELF and jumps to `_start` with `x0` = DTB physical address.
The DTB parser populates `BootInfo` from the live device tree, so
console / GIC / timer / RAM / firmware-framebuffer addresses come from
firmware rather than QEMU defaults.

### Path C — UEFI

1. The firmware loads `efi-stub/` (`hyperion-efi-stub.efi`) as a normal
   EFI application from `\EFI\BOOT\BOOTAA64.EFI`.
2. The stub uses Boot Services to discover hardware:
   - `LocateProtocol(EFI_GRAPHICS_OUTPUT_PROTOCOL)` → framebuffer
     base, size, resolution, stride, pixel format.
   - `GetMemoryMap` → conventional RAM regions for the PMM.
3. The stub loads the embedded kernel ELF into its fixed physical
   segments, then paints a recognisable test pattern into the framebuffer
   to prove GOP is writable (`make run-efi`).
4. It builds a UEFI handoff block from GOP + memory-map data, calls
   `ExitBootServices`, disables firmware MMU/cache state, and jumps to
   the kernel `_start` with the handoff pointer in `x0`.

## Memory layout

The fixed-address regions below describe the **QEMU virt** path; on
real boards the same regions live wherever `BootInfo` says they live.

```
0x0900_0000        PL011 UART (MMIO)            (QEMU virt default)
0x0800_0000..      GIC distributor               (QEMU virt default)
0x0801_0000..      GIC v2 CPU interface          (QEMU virt default)

0x4008_0000        kernel ELF load address (.text)
0x4009_x000        .rodata
0x4009_a000        .data + .bss + .stack + .heap (NOLOAD reserved)
0x400a_e000        kernel heap (4 MiB)
0x40b0_0000+       PMM-managed RAM (free for kernel allocations)
```

Kernel currently runs **identity-mapped with the MMU off**. Page tables
exist (`kernel/src/arch/aarch64/mmu.rs`, two-level 2 MiB block map covering
the first 1 GiB) but `enable()` is gated and only used by future EL0 work.

## Drivers

### UART

`kernel/src/drivers/uart/` exposes a `Console` trait and three drivers
selected at boot via the DTB:

- `pl011` — ARM PrimeCell PL011. QEMU virt, NXP i.MX, most server-class
  SoCs.
- `ns16550` — 8250/16550-compatible. Configurable register stride
  (`ns16550-stride`) covers Allwinner H6, NXP Layerscape, Marvell
  Armada, Rockchip, and most U-Boot-driven boards.
- `bcm_mini` — Broadcom BCM2835/BCM2837 mini-UART (Pi 3+ in
  baremetal mode).

The selected driver is wrapped in `BootConsole` and stored in a
`Mutex<Option<BootConsole>>` so `print!` / `println!` macros work from
anywhere.

### Interrupt controller

`kernel/src/arch/aarch64/gic/`:

- `v2` — memory-mapped distributor + CPU interface (QEMU virt, Cortex-
  A53/A72 reference platforms).
- `v3` — memory-mapped distributor + system-register CPU interface
  (`ICC_SRE_EL1`, `ICC_PMR_EL1`, `ICC_IGRPEN1_EL1`, `ICC_IAR1_EL1`,
  `ICC_EOIR1_EL1`). Found on Ampere, Cavium ThunderX, NXP LX2160,
  most modern server boards.

`gic::init` / `enable_ppi` / `handle_irq` dispatch on
`hal::info().gic.version`.

## Memory management

- **PMM** (`kernel/src/mm/pmm.rs`) — bitmap frame allocator, 4 KiB pages,
  256 MiB region. Reports total / free in KiB to the `mem` shell command.
- **Heap** (`kernel/src/mm/heap.rs`) — `linked_list_allocator::LockedHeap`
  global allocator over a 4 MiB region in `.heap`.
- **VMM** (`kernel/src/mm/vmm.rs`) — page-granular virtual address-space
  mappings with overlap validation and physical translation helpers.

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
  with **two backings**:
  - `Heap(Vec<u8>)` — owned heap pixels, used for virtual monitors.
  - `Mmio { base, len }` — raw MMIO pointer to firmware-provided
    framebuffer memory (UEFI GOP, simple-framebuffer, etc.).

  Both backings expose the same `pixels()` / `pixels_mut()` accessors,
  so all the drawing primitives (`clear`, `put_pixel`, `fill_rect`,
  `stroke_rect`) work transparently against either backing.
- `Monitor` (`kernel/src/display/monitor.rs`) — wraps a `Framebuffer`
  and tags it `Physical` or `Virtual`. `register_monitor` returns a
  monotonically allocated `MonitorId`.
- `Compositor` (`kernel/src/display/compositor.rs`) — z-ordered list of
  layers; `render()` walks layers low-z first and alpha-blends each into
  the destination. Used by the `demo` shell command.

`display::init()` checks `hal::info().framebuffer`:

- If a firmware framebuffer is reported (UEFI GOP / `simple-framebuffer`
  DT node), it wraps the MMIO range with `Framebuffer::from_mmio` and
  registers it as **physical** monitor `fb0`.
- Otherwise it registers a `1280x720` heap-backed **virtual** monitor
  (`monitor0`) so the rest of the system has somewhere to draw.

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
