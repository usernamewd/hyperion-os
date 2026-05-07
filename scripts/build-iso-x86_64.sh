#!/usr/bin/env bash
#
# Build a firmware-specific x86_64 bootable ISO from the Hyperion kernel.
#
# Usage:
#   scripts/build-iso-x86_64.sh bios [out.iso]
#   scripts/build-iso-x86_64.sh uefi [out.iso]
#
# Requirements:
#   grub-mkrescue, xorriso, cargo, and the matching GRUB platform modules.
#   On Debian/Ubuntu:
#     sudo apt-get install grub-pc-bin grub-efi-amd64-bin grub-common xorriso
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
STAGE="target/iso-stage-x86_64-${MODE}"
VOLID="HYPERION_${MODE^^}"

for tool in grub-mkrescue xorriso cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-iso-x86_64: missing required tool: $tool" >&2
        exit 1
    fi
done

case "$MODE" in
    bios)
        GRUB_PLATFORM_DIR="/usr/lib/grub/i386-pc"
        ;;
    uefi)
        GRUB_PLATFORM_DIR="/usr/lib/grub/x86_64-efi"
        ;;
esac

if [[ ! -d "$GRUB_PLATFORM_DIR" ]]; then
    echo "build-iso-x86_64: missing GRUB platform modules: $GRUB_PLATFORM_DIR" >&2
    exit 1
fi

echo "==> building x86_64 kernel"
cargo build -p hyperion-kernel --target "$KERNEL_TARGET" --release

if [[ ! -f "$KERNEL_BIN" ]]; then
    echo "build-iso-x86_64: kernel ELF not found at $KERNEL_BIN" >&2
    exit 1
fi

echo "==> staging ${MODE} GRUB tree at $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/boot/grub"
cp "$KERNEL_BIN" "$STAGE/boot/hyperion-kernel"

cat > "$STAGE/boot/grub/grub.cfg" <<EOF
set timeout=0
set default=0

menuentry "Hyperion OS (${MODE^^})" {
    multiboot2 /boot/hyperion-kernel
    boot
}

menuentry "Hyperion OS (${MODE^^}, text mode, no fb)" {
    multiboot2 /boot/hyperion-kernel nofb
    boot
}
EOF

cat > "$STAGE/README.TXT" <<EOF
Hyperion OS x86_64 ${MODE^^} boot ISO.

This image is intentionally firmware-specific:
  - bios: Legacy BIOS / CSM systems only
  - uefi: Modern x86_64 UEFI systems only

Use the matching ISO for your machine firmware.
EOF

echo "==> building ${MODE} ISO at $OUT"
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
echo "    mode: $MODE"
echo "    size: $ISO_SIZE bytes ($((ISO_SIZE / 1024)) KiB)"
echo
if [[ "$MODE" == "bios" ]]; then
    echo "Boot it under QEMU (BIOS):"
    echo "    qemu-system-x86_64 -m 512M -serial stdio -display none -cdrom $OUT"
else
    echo "Boot it under QEMU (UEFI/OVMF):"
    echo "    qemu-system-x86_64 -m 512M -serial stdio -display none \\"
    echo "        -bios /usr/share/OVMF/OVMF_CODE.fd -cdrom $OUT"
fi
