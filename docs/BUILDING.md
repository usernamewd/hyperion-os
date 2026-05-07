# Building Hyperion

## Prerequisites

- **Rust 1.83.0** (pinned via `rust-toolchain.toml`). `rustup` will
  install the right toolchain automatically the first time you build.
- **`aarch64-unknown-none` target** —
  `rustup target add aarch64-unknown-none`.
- **QEMU 6.0+** with aarch64 system emulation
  (`qemu-system-aarch64`). Tested on QEMU 6.2 and 8.x.
- **A linker for aarch64.** The pinned `.cargo/config.toml` invokes
  `rust-lld` via `lld-link` so you don't need a system cross-linker, but
  installing `aarch64-linux-gnu-gcc` / `aarch64-elf-gcc` is harmless.

## Build

```sh
make build         # release kernel ELF at target/aarch64-unknown-none/release/hyperion-kernel
make debug         # dev profile
```

Or, with raw cargo:

```sh
cargo build -p hyperion-kernel --target aarch64-unknown-none --release
```

The kernel is a position-fixed ELF whose entry is `_start` at `0x40080000`.
QEMU's `-kernel` flag accepts ELF directly — no `objcopy` to `.bin` needed.

## Lint / format

```sh
make clippy        # cargo clippy -p hyperion-kernel ... -- -D warnings
make fmt-check     # cargo fmt --all -- --check
make fmt           # apply formatting
```

## Run under QEMU

```sh
make run           # serial-only, nographic
make run-gfx       # serial on stdio + virtio-gpu PCI
```

Internally:

```sh
qemu-system-aarch64 \
    -M virt -cpu cortex-a72 -smp 1 -m 512M \
    -nographic -semihosting \
    -kernel target/aarch64-unknown-none/release/hyperion-kernel
```

`Ctrl-A x` quits QEMU. The shell command `shutdown` invokes PSCI
`SYSTEM_OFF`, which QEMU honours and exits cleanly.

## Inspecting the ELF

```sh
aarch64-linux-gnu-readelf -h target/aarch64-unknown-none/release/hyperion-kernel
aarch64-linux-gnu-nm     target/aarch64-unknown-none/release/hyperion-kernel | grep _start
```

Useful symbols:

| Symbol                            | Meaning                                  |
|-----------------------------------|------------------------------------------|
| `_start`                          | aarch64 boot entry (0x40080000)          |
| `__vectors` / `__early_vectors`   | EL1 exception vector tables              |
| `__bss_start` / `__bss_end`       | BSS range zeroed by `_start`             |
| `__stack_top`                     | Top of the boot stack                    |
| `__heap_start` / `__heap_end`     | Kernel heap region (4 MiB)               |

## Troubleshooting

- **No output, QEMU just hangs.** Almost always an early fault.
  Re-run with `-d cpu_reset,int,unimp,guest_errors -D /tmp/qemu.log`
  and check `/tmp/qemu.log`. The exception class in `ESR_EL1` tells
  you whether it was undefined-instruction (FP not enabled) vs page
  fault vs alignment.
- **`error: linking with cc failed`.** Make sure
  `aarch64-unknown-none` is installed and that `rust-lld` resolves; see
  `.cargo/config.toml` for the linker arguments we pass.
- **Older QEMU loops on boot.** Pre-6.0 QEMU virt mis-handles the EL2/EL1
  transition for cortex-a72; please upgrade.
