#!/usr/bin/env bash
#
# Build a raw GPT-partitioned disk image of Hyperion that is directly
# flashable to a USB stick (Rufus "DD mode", `dd`, balenaEtcher).
#
# Layout:
#
#   LBA 0      : protective MBR
#   LBA 1+2    : GPT primary header + entries
#   2 MiB ..   : EFI System Partition (FAT32, type EF00)
#                contains /EFI/BOOT/BOOTAA64.EFI
#   tail       : GPT secondary header + entries
#
# Unlike the ISO, this is a real disk image — UEFI firmware sees a
# proper GPT, finds the ESP by partition type, and loads
# BOOTAA64.EFI as the fallback boot loader.
#
# Usage:
#   scripts/build-usb-img.sh [out.img]
#
# Requirements:
#   sgdisk (gdisk pkg), mtools, dosfstools, cargo.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-target/hyperion-usb.img}"
EFI_TARGET="aarch64-unknown-uefi"
EFI_PROFILE="release"
EFI_BIN="target/${EFI_TARGET}/${EFI_PROFILE}/hyperion-efi-stub.efi"
STAGE_FAT="target/usb-stage/esp.img"
SECTOR=512
START_SECTOR=2048           # 1 MiB align
ESP_SIZE_MIB=64
ESP_SECTORS=$(( ESP_SIZE_MIB * 1024 * 1024 / SECTOR ))
END_SECTOR=$(( START_SECTOR + ESP_SECTORS - 1 ))
TAIL_SECTORS=2048           # leave room for GPT secondary
DISK_SECTORS=$(( END_SECTOR + TAIL_SECTORS + 1 ))
DISK_BYTES=$(( DISK_SECTORS * SECTOR ))

for tool in sgdisk mmd mcopy mkfs.vfat cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-usb-img: missing required tool: $tool" >&2
        exit 1
    fi
done

echo "==> building EFI stub"
cargo build -p hyperion-efi-stub --target "$EFI_TARGET" --release

if [[ ! -f "$EFI_BIN" ]]; then
    echo "build-usb-img: EFI stub not found at $EFI_BIN" >&2
    exit 1
fi

echo "==> staging ${ESP_SIZE_MIB} MiB ESP FAT image"
mkdir -p "$(dirname "$STAGE_FAT")"
rm -f "$STAGE_FAT"
truncate -s "${ESP_SIZE_MIB}MiB" "$STAGE_FAT"
mkfs.vfat -F 32 -n "HYPERION" "$STAGE_FAT" >/dev/null

mmd -i "$STAGE_FAT" ::EFI ::EFI/BOOT
mcopy -i "$STAGE_FAT" "$EFI_BIN" "::EFI/BOOT/BOOTAA64.EFI"

echo "==> creating ${DISK_SECTORS}-sector GPT disk image"
mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
truncate -s "$DISK_BYTES" "$OUT"

# Lay down a fresh GPT and create the ESP partition pointing at the
# region we'll splice the FAT image into.
sgdisk \
    --clear \
    --new=1:${START_SECTOR}:${END_SECTOR} \
    --change-name=1:"EFI System Partition" \
    --typecode=1:EF00 \
    "$OUT" >/dev/null

echo "==> splicing FAT into partition 1 at LBA ${START_SECTOR}"
dd if="$STAGE_FAT" of="$OUT" \
    bs="$SECTOR" seek="$START_SECTOR" count="$ESP_SECTORS" \
    conv=notrunc status=none

DISK_SIZE=$(stat -c '%s' "$OUT")
echo "==> done"
echo "    img:  $OUT"
echo "    size: $DISK_SIZE bytes ($((DISK_SIZE / 1024 / 1024)) MiB)"
echo
echo "Boot it under QEMU + AAVMF:"
echo "    make run-usb"
echo
echo "Flash to USB:"
echo "    Linux:   sudo dd if=$OUT of=/dev/sdX bs=4M status=progress conv=fsync"
echo "    Windows: Rufus -> Select $OUT -> 'Write in DD Image mode'"
echo "    Mac:     sudo dd if=$OUT of=/dev/rdiskN bs=4m"
