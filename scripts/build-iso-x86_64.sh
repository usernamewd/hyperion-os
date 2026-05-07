#!/usr/bin/env bash
#
# Build a firmware-specific x86_64 bootable ISO for Hyperion.
#
# Two modes:
#
#   bios  - Legacy BIOS / CSM path. We delegate to grub-mkrescue,
#           which embeds GRUB-PC and Hyperion's kernel ELF (loaded
#           via Multiboot2). This is unchanged from the original
#           script: BIOS firmware doesn't run UEFI EFI applications.
#
#   uefi  - Native UEFI path. We embed Hyperion's own EFI stub
#           (efi-stub/, x86_64-unknown-uefi target) at
#           /EFI/BOOT/BOOTX64.EFI inside an ESP, and lay it out as
#           an "El Torito" no-emul ISO with an appended GPT ESP — same
#           topology as the aarch64 ISO. This boots Hyperion under
#           OVMF / any modern x86_64 UEFI firmware *without* GRUB in
#           the chain: the EFI stub itself is the loader and hands
#           the kernel a populated UefiHandoff block.
#
# Usage:
#   scripts/build-iso-x86_64.sh bios [out.iso]
#   scripts/build-iso-x86_64.sh uefi [out.iso]
#
# Requirements (BIOS mode):
#   grub-mkrescue, xorriso, cargo, /usr/lib/grub/i386-pc.
#   On Debian/Ubuntu:
#     sudo apt-get install grub-pc-bin grub-common xorriso
#
# Requirements (UEFI mode):
#   xorriso, mtools (mformat, mmd, mcopy), dosfstools (mkfs.vfat),
#   cargo, the x86_64-unknown-uefi rustup target.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="${1:-uefi}"
if [[ "$MODE" != "bios" && "$MODE" != "uefi" ]]; then
    echo "build-iso-x86_64: first argument must be 'bios' or 'uefi'" >&2
    exit 1
fi

DEFAULT_OUT="target/hyperion-x86_64-${MODE}.iso"
OUT="${2:-$DEFAULT_OUT}"
KERNEL_TARGET="x86_64-unknown-none"
KERNEL_PROFILE="release"
KERNEL_BIN="target/${KERNEL_TARGET}/${KERNEL_PROFILE}/hyperion-kernel"
EFI_TARGET="x86_64-unknown-uefi"
EFI_BIN="target/${EFI_TARGET}/${KERNEL_PROFILE}/hyperion-efi-stub.efi"
VOLID="HYPERION_${MODE^^}"

build_kernel() {
    echo "==> building x86_64 kernel"
    cargo build -p hyperion-kernel --target "$KERNEL_TARGET" --release
    if [[ ! -f "$KERNEL_BIN" ]]; then
        echo "build-iso-x86_64: kernel ELF not found at $KERNEL_BIN" >&2
        exit 1
    fi
}

build_bios() {
    for tool in grub-mkrescue xorriso cargo; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "build-iso-x86_64: missing required tool: $tool" >&2
            exit 1
        fi
    done

    GRUB_PLATFORM_DIR="/usr/lib/grub/i386-pc"
    if [[ ! -d "$GRUB_PLATFORM_DIR" ]]; then
        echo "build-iso-x86_64: missing GRUB platform modules: $GRUB_PLATFORM_DIR" >&2
        exit 1
    fi

    build_kernel

    STAGE="target/iso-stage-x86_64-bios"
    echo "==> staging BIOS GRUB tree at $STAGE"
    rm -rf "$STAGE"
    mkdir -p "$STAGE/boot/grub"
    cp "$KERNEL_BIN" "$STAGE/boot/hyperion-kernel"

    cat > "$STAGE/boot/grub/grub.cfg" <<'EOF'
set timeout=0
set default=0

menuentry "Hyperion OS (BIOS)" {
    multiboot2 /boot/hyperion-kernel
    boot
}

menuentry "Hyperion OS (BIOS, text mode, no fb)" {
    multiboot2 /boot/hyperion-kernel nofb
    boot
}
EOF

    cat > "$STAGE/README.TXT" <<'EOF'
Hyperion OS x86_64 BIOS boot ISO.

This ISO is intended for legacy BIOS / CSM systems. GRUB-PC is
embedded; it loads the Hyperion kernel via Multiboot2 and hands
control to the kernel's _start (32-bit protected mode entry).

For UEFI x86_64 systems use the matching `-uefi` ISO instead.
EOF

    echo "==> building BIOS ISO at $OUT"
    mkdir -p "$(dirname "$OUT")"
    grub-mkrescue \
        -d "$GRUB_PLATFORM_DIR" \
        --output="$OUT" \
        "$STAGE" \
        -- \
        -volid "$VOLID" \
        2>&1 | sed 's/^/    /'

    ISO_SIZE=$(stat -c '%s' "$OUT")
    echo "==> done"
    echo "    iso:  $OUT"
    echo "    mode: bios"
    echo "    size: $ISO_SIZE bytes ($((ISO_SIZE / 1024)) KiB)"
    echo
    echo "Boot it under QEMU (BIOS):"
    echo "    qemu-system-x86_64 -m 512M -serial stdio -display none -cdrom $OUT"
}

build_uefi() {
    for tool in xorriso mmd mcopy mkfs.vfat cargo; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "build-iso-x86_64: missing required tool: $tool" >&2
            exit 1
        fi
    done

    build_kernel

    echo "==> building x86_64 EFI stub with embedded kernel"
    HYPERION_KERNEL_ELF="$ROOT/$KERNEL_BIN" \
        cargo build -p hyperion-efi-stub --target "$EFI_TARGET" --release

    if [[ ! -f "$EFI_BIN" ]]; then
        echo "build-iso-x86_64: EFI stub not found at $EFI_BIN" >&2
        exit 1
    fi

    STAGE="target/iso-stage-x86_64-uefi"
    ESP_IMG="$STAGE/EFI/efiboot.img"
    echo "==> staging ESP at $STAGE"
    rm -rf "$STAGE"
    mkdir -p "$(dirname "$ESP_IMG")"

    # Embedded kernel + UEFI loader headroom.
    ESP_SIZE_KIB=16384
    truncate -s "${ESP_SIZE_KIB}KiB" "$ESP_IMG"
    mkfs.vfat -F 16 -n "$VOLID" "$ESP_IMG" >/dev/null

    mmd -i "$ESP_IMG" ::EFI ::EFI/BOOT
    mcopy -i "$ESP_IMG" "$EFI_BIN" "::EFI/BOOT/BOOTX64.EFI"

    mkdir -p "$STAGE/hyperion"
    cp "$EFI_BIN" "$STAGE/hyperion/BOOTX64.EFI"
    cat > "$STAGE/README.TXT" <<'EOF'
Hyperion OS x86_64 UEFI boot ISO.

This is a native UEFI boot path: there is NO GRUB in the chain.
UEFI firmware enumerates the ESP, finds /EFI/BOOT/BOOTX64.EFI, and
runs Hyperion's own EFI stub (efi-stub/), which loads the kernel
ELF, walks the EFI configuration table for ACPI 2.0 / DTB, and
hands control to the kernel's _start_uefi entry with a populated
UefiHandoff block.
EOF

    echo "==> building UEFI ISO at $OUT"
    mkdir -p "$(dirname "$OUT")"
    xorriso -as mkisofs \
        -V "$VOLID" \
        -r -J -joliet-long \
        -e EFI/efiboot.img \
        -no-emul-boot \
        -append_partition 2 0xef "$ESP_IMG" \
        -partition_cyl_align all \
        -o "$OUT" \
        "$STAGE" \
        2>&1 | sed 's/^/    /'

    ISO_SIZE=$(stat -c '%s' "$OUT")
    echo "==> done"
    echo "    iso:  $OUT"
    echo "    mode: uefi"
    echo "    size: $ISO_SIZE bytes ($((ISO_SIZE / 1024)) KiB)"
    echo
    echo "Boot it under QEMU + OVMF:"
    echo "    make ARCH=x86_64 run-uefi"
    echo
    echo "Flash to USB:"
    echo "    Linux:   sudo dd if=$OUT of=/dev/sdX bs=4M status=progress conv=fsync"
    echo "    Windows: Rufus -> Select $OUT -> 'Write in DD Image mode'"
    echo "    Mac:     sudo dd if=$OUT of=/dev/rdiskN bs=4m"
}

case "$MODE" in
    bios)
        build_bios
        ;;
    uefi)
        build_uefi
        ;;
esac
