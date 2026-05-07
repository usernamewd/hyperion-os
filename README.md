# Hyperion OS

A small, integratable **aarch64/x86_64 microkernel** written in Rust, designed
to be the foundation for higher-level OS / UI projects (think HarmonyOS-style
layered systems, but at a much smaller scale).

> Status: **educational / proof-of-concept**. Hyperion boots end-to-end on
> QEMU `virt` aarch64 and x86_64 (and is structured to boot on real DT- and
> UEFI-based ARM64 hardware via the same kernel ELF). It drops you into an
> interactive shell and exposes a stable C-ABI / Rust API surface that custom
> OSes and UI stacks can build on top of. It is **not** production grade.

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
  heap**, and page-granular process address-space mappings ready for EL0.
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
- **UEFI boot loader** — single `efi-stub/` crate that compiles for both
  `aarch64-unknown-uefi` (`BOOTAA64.EFI`) and `x86_64-unknown-uefi`
  (`BOOTX64.EFI`). It locates GOP, walks the EFI configuration table for
  ACPI 2.0 / DTB pointers, embeds and loads the kernel ELF, builds a
  unified `UefiHandoff` from the UEFI memory map, calls `ExitBootServices`,
  and jumps into the kernel's arch-appropriate UEFI entry. No GRUB in the
  chain on either architecture.
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
rustup target add aarch64-unknown-none aarch64-unknown-uefi \
    x86_64-unknown-none x86_64-unknown-uefi

# Build the kernel.
make build

# Boot it under QEMU (serial-only).
make run

# Or boot via UEFI firmware (AAVMF) end-to-end:
sudo apt install qemu-efi-aarch64        # Debian/Ubuntu
make run-efi

# Or build bootable ISO / USB images:
sudo apt install xorriso mtools dosfstools gdisk grub-pc-bin grub-efi-amd64-bin
make iso                     # aarch64: target/hyperion-aarch64-uefi.iso
make ARCH=x86_64 iso         # x86_64: BIOS (GRUB-PC) and UEFI (native) ISOs
make usb-img                 # aarch64 raw USB image (BOOTAA64.EFI)
make ARCH=x86_64 usb-img     # x86_64 raw USB image  (BOOTX64.EFI, native UEFI)
make run-iso                 # smoke-test the UEFI ISO under QEMU + UEFI firmware
make run-usb                 # smoke-test the USB image under QEMU + UEFI firmware
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
├── kernel/             # The microkernel itself (no_std, aarch64/x86_64)
│   ├── src/arch/       # aarch64 boot, exceptions, GIC v2/v3, timer, MMU, PSCI
│   ├── src/hal/        # BootInfo / DTB parser / Console trait
│   ├── src/drivers/    # UART drivers (PL011, 16550, BCM mini-UART)
│   ├── src/mm/         # PMM, heap, VMM address-space mappings
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
├── scripts/            # build-iso.sh / build-usb-img.sh (bootable media)
└── docs/               # Architecture, building, porting, integration, UI API
```

## Where it boots today

- **QEMU `virt`** aarch64 + x86_64 q35 (`-kernel`, no firmware) — primary
  development target.
- **QEMU `virt` + AAVMF UEFI firmware (aarch64)** — `efi-stub/` boots
  end-to-end, discovers GOP, loads the embedded kernel ELF, walks the
  EFI configuration table for DTB / ACPI 2.0, exits boot services, and
  hands a `UefiHandoff` block to `_start_secondary`'s sibling
  `hyperion_kernel_kmain_trampoline`.
- **QEMU `q35` + OVMF UEFI firmware (x86_64)** — same `efi-stub/` crate
  built for `x86_64-unknown-uefi`, ELF-loads the kernel, and tail-calls
  `_start_uefi` with the handoff pointer in `RDI`. No GRUB on this path.
- **ARM64 UEFI ISO / USB image** (`make iso` / `make usb-img`) —
  bootable on real ARM64 UEFI systems; the ISO/USB carries
  `BOOTAA64.EFI` directly.
- **x86_64 Legacy BIOS ISO** (`make ARCH=x86_64 iso-bios`) — GRUB-PC,
  Multiboot2 entry-address tag, kernel `_start` (32-bit protected mode).
- **x86_64 UEFI ISO** (`make ARCH=x86_64 iso-uefi`) — native Hyperion
  EFI stub baked into the ESP. Boots under any modern x86_64 UEFI
  firmware via `BOOTX64.EFI`.
- **x86_64 UEFI USB image** (`make ARCH=x86_64 usb-img`) — raw GPT
  disk image with native `BOOTX64.EFI` in the ESP. Flashable to USB
  with `dd` / Rufus (DD mode) / balenaEtcher.
- **DT-described ARM64 boards** with a PL011, 16550-compatible, or
  BCM mini-UART console and a GICv2 or GICv3 interrupt controller — once
  the bootloader hands the kernel a DTB and jumps to `_start`.

## Roadmap (post-MVP)

- virtio-gpu PCI driver + scanout flush (real graphical window without UEFI).
- Multi-core scheduler (parking is in place; SMP wakeup via PSCI `CPU_ON`).
- EL0 user processes wired through `SVC #0` (dispatcher exists, switching is
  feature-gated).
- Block-device-backed FS (virtio-blk → ext2/littlefs).
- Networking (virtio-net + smoltcp).

## License

Dual-licensed under MIT or Apache-2.0, at your option. See
[`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).
