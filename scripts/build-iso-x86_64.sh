#!/usr/bin/env bash
#
# Build a hybrid x86_64 BIOS + UEFI bootable ISO from the Hyperion
# kernel.
#
# The kernel is a standalone Multiboot2 ELF (`hyperion-kernel`) and we
# embed it into a GRUB2 rescue image alongside a `grub.cfg` that boots
# it via the multiboot2 command. The ISO is built with `grub-mkrescue`,
# which produces an El Torito CD that's also flashable to USB and
# bootable on both legacy BIOS and UEFI x86_64 firmware.
#
# Usage:
#   scripts/build-iso-x86_64.sh [out.iso]
#
# Requirements:
#   grub-mkrescue, xorriso, mtools (for the embedded FAT ESP).
#   On Debian/Ubuntu:
#     sudo apt-get install grub-pc-bin grub-efi-amd64-bin grub-common \
#                          xorriso mtools dosfstools
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-target/hyperion-x86_64.iso}"
KERNEL_TARGET="x86_64-unknown-none"
KERNEL_PROFILE="release"
KERNEL_BIN="target/${KERNEL_TARGET}/${KERNEL_PROFILE}/hyperion-kernel"
STAGE="target/iso-stage-x86_64"
VOLID="HYPERION"

for tool in grub-mkrescue xorriso cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-iso-x86_64: missing required tool: $tool" >&2
        exit 1
    fi
done

echo "==> building x86_64 kernel"
cargo build -p hyperion-kernel --target "$KERNEL_TARGET" --release

if [[ ! -f "$KERNEL_BIN" ]]; then
    echo "build-iso-x86_64: kernel ELF not found at $KERNEL_BIN" >&2
    exit 1
fi

echo "==> staging GRUB tree at $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/boot/grub"
cp "$KERNEL_BIN" "$STAGE/boot/hyperion-kernel"

cat > "$STAGE/boot/grub/grub.cfg" <<'EOF'
set timeout=0
set default=0

# Hyperion runs as a Multiboot2 payload. Both BIOS GRUB (i386-pc
# modules) and UEFI GRUB (x86_64-efi modules) use the same boot
# command, so a single ISO covers both firmware paths.
menuentry "Hyperion OS (Multiboot2)" {
    multiboot2 /boot/hyperion-kernel
    boot
}

menuentry "Hyperion OS (text mode, no fb)" {
    multiboot2 /boot/hyperion-kernel nofb
    boot
}
EOF

echo "==> building hybrid ISO at $OUT"
mkdir -p "$(dirname "$OUT")"
# Pass --volid through mkisofs's -V (grub-mkrescue forwards args after `--`
# straight to xorriso -as mkisofs); older xorriso builds don't recognise
# `--volid=` directly.
grub-mkrescue \
    --output="$OUT" \
    "$STAGE" \
    -- \
    -volid "$VOLID" \
    2>&1 | sed 's/^/    /'

ISO_SIZE=$(stat -c '%s' "$OUT")
echo "==> done"
echo "    iso:  $OUT"
echo "    size: $ISO_SIZE bytes ($((ISO_SIZE / 1024)) KiB)"
echo
echo "Boot it under QEMU (BIOS):"
echo "    qemu-system-x86_64 -m 512M -serial stdio -display none -cdrom $OUT"
echo
echo "Boot it under QEMU (UEFI/OVMF):"
echo "    qemu-system-x86_64 -m 512M -serial stdio -display none \\"
echo "        -bios /usr/share/OVMF/OVMF_CODE.fd -cdrom $OUT"
echo
echo "Flash to USB (Linux):"
echo "    sudo dd if=$OUT of=/dev/sdX bs=4M status=progress conv=fsync"
