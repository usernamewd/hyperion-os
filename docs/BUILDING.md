# Building Hyperion

## Prerequisites

- **Rust 1.83.0** (pinned via `rust-toolchain.toml`). `rustup` will
  install the right toolchain automatically the first time you build.
- **`aarch64-unknown-none` target** —
  `rustup target add aarch64-unknown-none`.
- **`aarch64-unknown-uefi` target** (only needed if you build the UEFI
  boot stub) — `rustup target add aarch64-unknown-uefi`.
- **QEMU 6.0+** with aarch64 system emulation
  (`qemu-system-aarch64`). Tested on QEMU 6.2 and 8.x.
- **A linker for aarch64.** The pinned `.cargo/config.toml` invokes
  `rust-lld` via `lld-link` so you don't need a system cross-linker, but
  installing `aarch64-linux-gnu-gcc` / `aarch64-elf-gcc` is harmless.
- **AAVMF UEFI firmware** (only needed for `make run-efi`) — on
  Debian/Ubuntu install `qemu-efi-aarch64`; on Fedora/RHEL install
  `edk2-aarch64`. The Makefile expects `/usr/share/AAVMF/AAVMF_CODE.fd`
  and `AAVMF_VARS.fd` by default; override `AAVMF_CODE` / `AAVMF_VARS`
  if your distro keeps them elsewhere.

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

### Direct `-kernel` boot (no firmware)

```sh
make run           # serial-only, nographic
make run-gfx       # serial on stdio + virtio-gpu PCI device
```

Internally:

```sh
qemu-system-aarch64 \
    -M virt -cpu cortex-a72 -smp 1 -m 512M \
    -nographic -semihosting \
    -kernel target/aarch64-unknown-none/release/hyperion-kernel
```

This path uses `BootInfo::qemu_virt_fallback()` — the compiled-in QEMU
defaults — so no DTB is needed.

`Ctrl-A x` quits QEMU. The shell command `shutdown` invokes PSCI
`SYSTEM_OFF`, which QEMU honours and exits cleanly.

### UEFI boot via AAVMF + the EFI stub

```sh
make efi           # build the .efi PE only
make esp           # build .efi and stage it under target/esp/EFI/BOOT/BOOTAA64.EFI
make run-efi       # build + run under QEMU + AAVMF UEFI firmware
```

`run-efi` boots OVMF/AAVMF firmware, which auto-loads
`EFI/BOOT/BOOTAA64.EFI` from the synthetic FAT image, locates a
`EFI_GRAPHICS_OUTPUT_PROTOCOL`, prints the discovered framebuffer
geometry to the UEFI serial console, and paints a Hyperion test
pattern into the framebuffer to prove the path is live.

Expected output on a fresh `make run-efi`:

```
BdsDxe: loading Boot0002 "UEFI Misc Device 2" from PciRoot(0x0)/Pci(0x?,0x0)
BdsDxe: starting Boot0002 ...

Hyperion EFI stub starting...
GOP framebuffer @ 0x0000000058430000 size=0x0000000000300000
  resolution=800x600 stride=800 pixfmt=1
Test pattern painted; halting.
```

The stub currently halts after painting; the kernel-handover patch
(`ExitBootServices` + jump to `kmain` with a populated `BootInfo`) is
the next iteration.

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
