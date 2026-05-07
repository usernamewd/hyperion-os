# Building Hyperion

## Prerequisites

- **Rust 1.83.0** (pinned via `rust-toolchain.toml`). `rustup` will
  install the right toolchain automatically the first time you build.
- **Rust targets** — `aarch64-unknown-none`, `aarch64-unknown-uefi`,
  `x86_64-unknown-none`, and `x86_64-unknown-uefi` are listed in
  `rust-toolchain.toml`; `rustup` installs them automatically.
- **QEMU 6.0+** with aarch64 system emulation
  (`qemu-system-aarch64`). Tested on QEMU 6.2 and 8.x.
- **A linker for aarch64.** The pinned `.cargo/config.toml` invokes
  `rust-lld` via `lld-link` so you don't need a system cross-linker, but
  installing `aarch64-linux-gnu-gcc` / `aarch64-elf-gcc` is harmless.
- **AAVMF UEFI firmware** (only needed for `make run-efi` /
  `make run-iso` / `make run-usb`) — on Debian/Ubuntu install
  `qemu-efi-aarch64`; on Fedora/RHEL install `edk2-aarch64`. The
  Makefile expects `/usr/share/AAVMF/AAVMF_CODE.fd` and
  `AAVMF_VARS.fd` by default; override `AAVMF_CODE` / `AAVMF_VARS`
  if your distro keeps them elsewhere.
- **ISO / USB image tooling** (only needed for `make iso` /
  `make usb-img`) — `xorriso`, `mtools`, `dosfstools`, `gdisk`
  (provides `sgdisk`), and GRUB platform modules for x86_64 media.
  On Debian/Ubuntu:
  `sudo apt-get install xorriso mtools dosfstools gdisk grub-pc-bin grub-efi-amd64-bin grub-common`.
  On Fedora/RHEL: install the matching `xorriso`, `mtools`,
  `dosfstools`, `gdisk`, and GRUB EFI/BIOS module packages.

## Build

```sh
make build              # release kernel ELF for ARCH (default: aarch64)
make ARCH=x86_64 build  # x86_64 release kernel ELF
make build-all          # release kernel ELFs for aarch64 and x86_64
make debug              # dev profile
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
`EFI_GRAPHICS_OUTPUT_PROTOCOL`, embeds and loads the kernel ELF,
exits boot services, and jumps into the kernel with a UEFI handoff
block containing RAM and framebuffer details.

Expected output on a fresh `make run-efi`:

```
BdsDxe: loading Boot0002 "UEFI Misc Device 2" from PciRoot(0x0)/Pci(0x?,0x0)
BdsDxe: starting Boot0002 ...

Hyperion EFI stub starting...
GOP framebuffer @ 0x0000000058430000 size=0x0000000000300000
  resolution=800x600 stride=800 pixfmt=1
Loading embedded kernel ELF...
Kernel loaded; exiting boot services.
============================================================
  Hyperion OS  --  aarch64 microkernel  v0.1.0
```

## Bootable ISO and USB image

Hyperion can be packaged into firmware-specific boot media. ARM64 media
is UEFI-only and wraps `BOOTAA64.EFI`; x86_64 builds produce separate
Legacy BIOS and UEFI ISOs instead of a combined hybrid image.

```sh
make iso                     # aarch64: target/hyperion-aarch64-uefi.iso
make ARCH=x86_64 iso         # x86_64: BIOS and UEFI ISOs
make ARCH=x86_64 iso-bios    # target/hyperion-x86_64-bios.iso
make ARCH=x86_64 iso-uefi    # target/hyperion-x86_64-uefi.iso
make usb-img                 # aarch64: target/hyperion-usb.img
```

| Artefact                            | Layout                                  | Use case                                |
|-------------------------------------|-----------------------------------------|-----------------------------------------|
| `target/hyperion-aarch64-uefi.iso`  | ARM64 UEFI ISO (ISO9660 + GPT)          | optical media, virtual CD, or USB flash |
| `target/hyperion-x86_64-bios.iso`   | x86_64 GRUB BIOS El Torito ISO          | Legacy BIOS / CSM systems               |
| `target/hyperion-x86_64-uefi.iso`   | x86_64 GRUB UEFI El Torito ISO          | Modern x86_64 UEFI systems              |
| `target/hyperion-usb.img`           | raw GPT-partitioned disk image with ESP | ARM64 UEFI USB / SD card                |

UEFI firmware on ARM64 finds and runs `/EFI/BOOT/BOOTAA64.EFI` from
the EFI System Partition embedded in the ISO or USB image. x86_64 UEFI
boots through GRUB EFI and x86_64 Legacy BIOS boots through GRUB BIOS.

### Flash the USB image

The raw `.img` is intentionally byte-identical to what should land on
the USB stick — no installer wrapper, no auto-extracting layer.

- **Windows (Rufus):** open Rufus → "SELECT" → pick
  `target/hyperion-usb.img`. When Rufus prompts for image mode, choose
  **"Write in DD Image mode"** (not ISO mode). Click "START".
- **Windows (Win32DiskImager / balenaEtcher):** open the tool, point
  it at `target/hyperion-usb.img`, pick the USB stick, write.
- **Linux:**
  ```sh
  sudo dd if=target/hyperion-usb.img of=/dev/sdX bs=4M \
      status=progress conv=fsync
  sync
  ```
- **macOS:**
  ```sh
  diskutil unmountDisk /dev/diskN
  sudo dd if=target/hyperion-usb.img of=/dev/rdiskN bs=4m
  ```

The UEFI ISOs can be flashed identically (`dd` / Rufus DD mode).
Rufus's "ISO mode" tries to repackage the ISO and may break firmware
boot files; always pick DD mode.

### Smoke-test the ISO and USB image under QEMU

```sh
make run-iso                 # boots target/hyperion-aarch64-uefi.iso under QEMU + AAVMF
make ARCH=x86_64 run-bios    # boots target/hyperion-x86_64-bios.iso under SeaBIOS
make ARCH=x86_64 run-uefi    # boots target/hyperion-x86_64-uefi.iso under OVMF
make run-usb                 # boots target/hyperion-usb.img under QEMU + AAVMF
```

Expected output on either target:

```
BdsDxe: starting Boot000? "UEFI ..."

Hyperion EFI stub starting...
GOP framebuffer @ 0x0000000058430000 size=0x0000000000300000
  resolution=800x600 stride=800 pixfmt=1
Loading embedded kernel ELF...
Kernel loaded; exiting boot services.
============================================================
  Hyperion OS  --  aarch64 microkernel  v0.1.0
```

If you see the kernel banner, the boot chain is healthy and the artefact will boot
on real ARM64 UEFI hardware too. (`Ctrl-A x` quits QEMU.)

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
