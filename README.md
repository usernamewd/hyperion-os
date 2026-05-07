# Hyperion OS

A small, integratable **aarch64 microkernel** written in Rust, designed to be the
foundation for higher-level OS / UI projects (think HarmonyOS-style layered
systems, but at a much smaller scale).

> Status: **educational / proof-of-concept**. Hyperion boots end-to-end on
> QEMU `virt` aarch64 (and is structured to boot on real DT- and UEFI-based
> ARM64 hardware via the same kernel ELF). It drops you into an interactive
> shell and exposes a stable C-ABI / Rust API surface that custom OSes and UI
> stacks can build on top of. It is **not** production grade.

## What's in the box

- **aarch64 boot stub** — drops from EL2 → EL1, parks secondaries, sets up
  vectors, enables FP/SIMD, hands off to Rust `kmain`. `x0` (DTB pointer) is
  preserved end to end.
- **Hardware Abstraction Layer** — `BootInfo` / `Console` traits decouple the
  kernel from any specific board. Includes a minimal **Flattened Device Tree
  (FDT v17) parser** that discovers RAM banks, console UART, GIC version, and
  firmware framebuffer at boot.
- **Multi-UART** — drivers for **PL011** (ARM PrimeCell), **8250 / 16550**
  (NXP Layerscape, Allwinner, Marvell, Rockchip, …) and the
  **BCM2835/2837 mini-UART** (Pi 3+). Selected by DTB `compatible` string.
- **GICv2 + GICv3** — both interrupt-controller generations supported. v2
  uses memory-mapped CPU interface; v3 uses the system-register interface
  (`ICC_*_EL1`).
- **Memory management** — bitmap **PMM** sized from `BootInfo`'s memory map,
  identity-mapped **MMU** plumbing, `linked_list_allocator`-backed **kernel
  heap**, VMM placeholder ready for EL0.
- **Microkernel core** — `Process` / `Thread` / `AddressSpace` types with a
  cooperative-preemptive **round-robin scheduler** driven by the CNTV timer.
- **IPC** — capability handles + bounded message **ports** (32 messages,
  64-byte payload + 8 × `u64` words).
- **Filesystem** — VFS trait layer + in-memory **ramfs** mounted at `/`,
  seeded with `/welcome` and `/version`.
- **Display subsystem** — generic `Framebuffer` with **dual backing** (heap
  `Vec<u8>` for virtual monitors; raw MMIO pointer for firmware-provided
  framebuffers like UEFI GOP / `simple-framebuffer`). `Monitor` abstraction
  (physical / virtual) and a stacking **compositor** with z-ordered alpha
  blending. Default boot registers a firmware monitor if available, else a
  `1280x720` virtual one.
- **UEFI boot stub** — separate `aarch64-unknown-uefi` PE/COFF EFI
  application (`efi-stub/`) that can be dropped into the ESP as
  `EFI/BOOT/BOOTAA64.EFI`. Currently locates GOP, prints framebuffer info to
  the firmware console, and paints a test pattern; kernel handover patch is
  the next step in this series.
- **UI building API** — `Canvas` (rect/line/text), an embedded **8×8 bitmap
  font**, and reusable `Widget` primitives (`Panel`, `Label`, `Button`).
- **Stable extension surface** — the [`hyperion-os-api`](./libos-api) crate
  re-exports the kernel-facing types so external `no_std` code (a custom OS,
  a UI shell, a test harness) can build against a versioned interface.
- **Shell** — interactive REPL over the boot UART with 16 built-in commands:
  `help`, `echo`, `clear`, `ls`, `cat`, `write`, `rm`, `mkdir`, `ps`, `mem`,
  `display`, `demo`, `uptime`, `history`, `reboot`, `shutdown`.

## Quickstart

```sh
# Toolchain — pinned via rust-toolchain.toml (Rust 1.83.0).
rustup target add aarch64-unknown-none
rustup target add aarch64-unknown-uefi   # only needed for `make efi`

# Build the kernel.
make build

# Boot it under QEMU (serial-only).
make run

# Or boot via UEFI firmware (AAVMF) end-to-end:
sudo apt install qemu-efi-aarch64        # Debian/Ubuntu
make run-efi
```

You should see:

```
============================================================
  Hyperion OS  --  aarch64 microkernel  v0.1.0
  (c) Hyperion contributors. MIT OR Apache-2.0.
============================================================

[ INFO] kmain: entering kernel main
[ INFO] memory: PMM ready, heap online
[ INFO] arch: exceptions + timer + GIC initialised
[ INFO] proc: scheduler initialised
[ INFO] services: ipc / fs / display initialised
[ INFO] shell spawned; handing control to the scheduler

Hyperion shell ready. Type 'help' for commands.

hyperion:/ $
```

Try `help`, `ls /`, `mem`, `ps`, `demo`, `uptime`, `cat /welcome`, `shutdown`.

To exit QEMU at any time: `Ctrl-A x`.

## Documentation

- [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) — component tour, boot flow, HAL, scheduler, IPC.
- [`docs/BUILDING.md`](./docs/BUILDING.md) — toolchain, targets, QEMU + UEFI run options.
- [`docs/PORTING.md`](./docs/PORTING.md) — adding support for a new ARM64 SoC / board.
- [`docs/INTEGRATION.md`](./docs/INTEGRATION.md) — how to build a custom OS / shell on top of Hyperion using `libos-api`.
- [`docs/UI_API.md`](./docs/UI_API.md) — using the framebuffer, compositor, canvas, and widgets.

## Repository layout

```
hyperion-os/
├── kernel/             # The microkernel itself (no_std, aarch64-unknown-none)
│   ├── src/arch/       # aarch64 boot, exceptions, GIC v2/v3, timer, MMU, PSCI
│   ├── src/hal/        # BootInfo / DTB parser / Console trait
│   ├── src/drivers/    # UART drivers (PL011, 16550, BCM mini-UART)
│   ├── src/mm/         # PMM, heap, VMM placeholder
│   ├── src/proc/       # threads, processes, scheduler, context switch
│   ├── src/ipc/        # capability table + message ports
│   ├── src/fs/         # VFS trait + ramfs
│   ├── src/display/    # framebuffer (heap+MMIO), monitors, compositor
│   ├── src/ui/         # canvas, font, widgets
│   ├── src/syscall/    # SVC dispatch (scaffolding)
│   ├── src/shell/      # interactive REPL
│   └── linker.ld
├── efi-stub/           # aarch64-unknown-uefi UEFI boot stub (.efi)
├── libos-api/          # Stable, no_std-friendly public surface
└── docs/               # Architecture, building, porting, integration, UI API
```

## Where it boots today

- **QEMU `virt`** (`-kernel`, no firmware) — primary development target.
- **QEMU `virt` + AAVMF UEFI firmware** — `efi-stub/` boots end-to-end,
  discovers the UEFI GOP framebuffer, paints a test pattern. Kernel
  handover from the stub is the next iteration.
- **DT-described ARM64 boards** with a PL011, 16550-compatible, or
  BCM mini-UART console and a GICv2 or GICv3 interrupt controller — should
  work once the bootloader hands the kernel a DTB and jumps to `_start`.

## Roadmap (post-MVP)

- UEFI handover wiring: stub builds a `BootInfo`, calls `ExitBootServices`,
  loads the kernel ELF and jumps with the populated `BootInfo` pre-installed.
- virtio-gpu PCI driver + scanout flush (real graphical window without UEFI).
- Multi-core scheduler (parking is in place; SMP wakeup via PSCI `CPU_ON`).
- EL0 user processes wired through `SVC #0` (dispatcher exists, switching is
  feature-gated).
- Block-device-backed FS (virtio-blk → ext2/littlefs).
- Networking (virtio-net + smoltcp).

## License

Dual-licensed under MIT or Apache-2.0, at your option. See
[`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).
