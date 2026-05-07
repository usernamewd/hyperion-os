# Hyperion OS

A small, integratable **aarch64 microkernel** written in Rust, designed to be the
foundation for higher-level OS / UI projects (think HarmonyOS-style layered
systems, but at a much smaller scale).

> Status: **educational / proof-of-concept**. Hyperion boots end-to-end on
> QEMU `virt` aarch64, drops you into an interactive shell, and exposes a
> stable C-ABI / Rust API surface that custom OSes and UI stacks can build on
> top of. It is **not** production grade.

## What's in the box

- **aarch64 boot stub** — drops from EL2 → EL1, parks secondaries, sets up
  vectors, enables FP/SIMD, hands off to Rust `kmain`.
- **Memory management** — bitmap **PMM**, identity-mapped **MMU** plumbing,
  `linked_list_allocator`-backed **kernel heap**, virtual-memory placeholder
  ready for EL0.
- **Microkernel core** — `Process` / `Thread` / `AddressSpace` types with a
  cooperative-preemptive **round-robin scheduler** driven by the CNTV timer.
- **IPC** — capability handles + bounded message **ports** (32 messages,
  64-byte payload + 8 × `u64` words).
- **Filesystem** — VFS trait layer + in-memory **ramfs** mounted at `/`,
  seeded with `/welcome` and `/version`.
- **Display subsystem** — generic `Framebuffer`, `Monitor` abstraction
  (physical / virtual), and a stacking **compositor** with z-ordered alpha
  blending. Default boot creates a `1280x720` virtual monitor.
- **UI building API** — `Canvas` (rect/line/text), an embedded **8×8 bitmap
  font**, and reusable `Widget` primitives (`Panel`, `Label`, `Button`).
- **Stable extension surface** — the [`hyperion-os-api`](./libos-api) crate
  re-exports the kernel-facing types so external `no_std` code (a custom OS,
  a UI shell, a test harness) can build against a versioned interface.
- **Shell** — interactive REPL over PL011 UART with 16 built-in commands:
  `help`, `echo`, `clear`, `ls`, `cat`, `write`, `rm`, `mkdir`, `ps`, `mem`,
  `display`, `demo`, `uptime`, `history`, `reboot`, `shutdown`.

## Quickstart

```sh
# Toolchain — pinned via rust-toolchain.toml (Rust 1.83.0).
rustup target add aarch64-unknown-none

# Build the kernel.
make build

# Boot it under QEMU (serial-only).
make run
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

## Repository layout

```
hyperion-os/
├── kernel/             # The microkernel itself (no_std, aarch64-unknown-none)
│   ├── src/arch/       # aarch64 boot, exceptions, GIC, timer, MMU, UART, PSCI
│   ├── src/mm/         # PMM, heap, VMM placeholder
│   ├── src/proc/       # threads, processes, scheduler, context switch
│   ├── src/ipc/        # capability table + message ports
│   ├── src/fs/         # VFS trait + ramfs
│   ├── src/display/    # framebuffer, monitors, compositor
│   ├── src/ui/         # canvas, font, widgets
│   ├── src/syscall/    # SVC dispatch (scaffolding)
│   ├── src/shell/      # interactive REPL
│   └── linker.ld
├── libos-api/          # Stable, no_std-friendly public surface
└── docs/               # Architecture, building, integration, UI API guides
```

## Documentation

- [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) — component tour, boot flow, scheduler, IPC.
- [`docs/BUILDING.md`](./docs/BUILDING.md) — toolchain, targets, QEMU options.
- [`docs/INTEGRATION.md`](./docs/INTEGRATION.md) — how to build a custom OS / shell on top of Hyperion using `libos-api`.
- [`docs/UI_API.md`](./docs/UI_API.md) — using the framebuffer, compositor, canvas, and widgets.

## Roadmap (post-MVP)

- Multi-core scheduler (parking is in place; SMP wakeup via PSCI `CPU_ON`).
- EL0 user processes wired through `SVC #0` (dispatcher exists, switching is
  feature-gated).
- Block-device-backed FS (virtio-blk → ext2/littlefs).
- virtio-input + virtio-gpu integration so the compositor can drive a real
  framebuffer instead of a virtual monitor.
- Networking (virtio-net + smoltcp).

## License

Dual-licensed under MIT or Apache-2.0, at your option. See
[`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).
