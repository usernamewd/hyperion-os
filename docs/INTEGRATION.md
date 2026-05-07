# Integrating Hyperion: building custom OSes / UI stacks on top

Hyperion exposes its public, versioned surface through the
[`hyperion-os-api`](../libos-api) crate (`hyperion_os_api` once imported).
That crate is `no_std`-friendly and has no dependency on the kernel
itself, so it can be pulled in by:

- another `no_std` aarch64 binary that you cross-compile and load with
  `-kernel` instead of Hyperion (a bring-your-own-kernel scenario);
- an EL0 user payload that you load through Hyperion's eventual exec
  path and which talks to the kernel via `SVC #0`;
- a host-side test harness (build with `--features std`).

## Adding it as a dependency

```toml
[dependencies]
hyperion-os-api = { path = "../hyperion-os/libos-api", default-features = false }

# or, when your code can use std (host tests):
# hyperion-os-api = { path = "../hyperion-os/libos-api", features = ["std"] }
```

## What's exported

```rust
use hyperion_os_api::{
    VERSION,                       // crate version string
    syscall::{SyscallNr, raw_syscall},
    fs::{FileType, FsError, OpenFlags},
    display::{PixelFormat, MonitorKind, MonitorInfo, Colour},
    ui::{Rect, CanvasOps},
};
```

### `syscall`

The system-call ABI is documented as a single `enum SyscallNr`:

| Number | Variant     | Effect                                                 |
|-------:|-------------|--------------------------------------------------------|
| 0      | `Yield`     | Voluntarily yield to the scheduler.                    |
| 1      | `Exit`      | Exit the current thread (`x0` = exit code).            |
| 2      | `PutChar`   | Write `x0` as a byte to the kernel UART.               |
| 3      | `GetChar`   | Read one byte from the UART (blocking).                |
| 4      | `Uptime`    | Returns ticks-since-boot in `x0`.                      |
| 5      | `Reboot`    | PSCI `SYSTEM_RESET`.                                   |
| 6      | `Shutdown`  | PSCI `SYSTEM_OFF`.                                     |

`raw_syscall` issues the `svc #0` instruction. From EL1 (where current
Hyperion services live) the same calls can be done directly through the
kernel modules; the syscall numbers exist so EL0 user code can use the
identical interface once that path is enabled.

### `fs`

`FileType`, `FsError`, and `OpenFlags` mirror the kernel-side definitions
exactly. A user-space FS client only needs these types plus the
`open`/`read`/`write` syscalls (TBD; ports are the current alternative).

### `display`

`MonitorInfo` describes one monitor (`id`, `width`, `height`, `kind`,
`format`). `Colour` is a tiny RGBA helper with named constants
(`BLACK`, `WHITE`, `RED`, `GREEN`, `BLUE`).

### `ui`

`Rect { x, y, w, h }` and the `CanvasOps` trait define the minimum any
canvas must implement (`clear`, `fill_rect`, `stroke_rect`, `line`,
`text`). The kernel-side `Canvas` (in `kernel/src/ui/canvas.rs`)
satisfies the same shape; an external compositor / shell can implement
`CanvasOps` against its own backing store.

## Building your own OS shell

The simplest custom OS is just **another shell**: replace the spawn of
`hyperion::shell::spawn()` in `kernel/src/lib.rs` with a call into your
own crate. You can:

1. Add your crate to the workspace and `extern crate my_shell`.
2. In your shell:

```rust
#![no_std]
use hyperion_os_api::ui::{CanvasOps, Rect};
use hyperion_os_api::display::Colour;

pub fn run<C: CanvasOps>(canvas: &mut C, screen: Rect) {
    canvas.clear(Colour::BLACK.0);
    canvas.fill_rect(screen.x, screen.y, screen.w, screen.h, Colour::BLUE.0);
    canvas.text(20, 20, "MyOS", Colour::WHITE.0);
}
```

3. The kernel hands you an in-memory `Canvas` for the default monitor
   via `display::get(0).with_framebuffer(|fb| { let mut c = Canvas::new(fb); ... })`.

Once SVC dispatch covers all the operations your shell needs, you can
move it to EL0 by linking against `hyperion-os-api` only and using
`raw_syscall` instead of touching kernel modules directly.

## Backwards-compat policy

`libos-api` follows semver:

- **0.x.y** — anything can move; you should pin an exact version.
- **1.0+** — breaking changes only on major bumps; new syscalls or
  `MonitorInfo` fields will be additive.

The kernel ships its own version of the API but always with a matching
`hyperion-os-api` re-export, so external code never has to depend on
the kernel crate itself.
