#!/usr/bin/env bash
#
# Build a hybrid ARM64 UEFI bootable ISO from the Hyperion EFI stub.
#
# The ISO is a "no-emul" El Torito + appended-GPT hybrid:
#
#   - ISO9660 filesystem at the top level (so it mounts as a CD).
#   - Embedded FAT image acting as the EFI System Partition (ESP),
#     containing /EFI/BOOT/BOOTAA64.EFI (the UEFI fallback boot path
#     for ARM64 — UEFI firmware looks for this exact path on any
#     attached removable media).
#   - The same FAT image is also exposed via xorriso's
#     `-append_partition 2 0xef ...`, which appends a real GPT
#     partition pointing at the embedded ESP. UEFI ARM64 firmware
#     can therefore find the ESP either through El Torito boot
#     (CD path) or through plain GPT enumeration (USB / dd path).
#
# Usage:
#   scripts/build-iso.sh [out.iso]
#
# Requirements:
#   xorriso, mtools (mformat, mmd, mcopy), dosfstools (mkfs.vfat).
#   The Hyperion EFI stub is built first (cargo build).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-target/hyperion.iso}"
EFI_TARGET="aarch64-unknown-uefi"
EFI_PROFILE="release"
EFI_BIN="target/${EFI_TARGET}/${EFI_PROFILE}/hyperion-efi-stub.efi"
STAGE="target/iso-stage"
ESP_IMG="target/iso-stage/EFI/efiboot.img"
VOLID="HYPERION"

for tool in xorriso mmd mcopy mkfs.vfat cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-iso: missing required tool: $tool" >&2
        exit 1
    fi
done

echo "==> building EFI stub"
cargo build -p hyperion-efi-stub --target "$EFI_TARGET" --release

if [[ ! -f "$EFI_BIN" ]]; then
    echo "build-iso: EFI stub not found at $EFI_BIN" >&2
    exit 1
fi

echo "==> staging ESP at $STAGE"
rm -rf "$STAGE"
mkdir -p "$(dirname "$ESP_IMG")"

# 4 MiB FAT image is plenty for a single 50 KiB BOOTAA64.EFI; gives
# headroom for a kernel + extras in later iterations.
ESP_SIZE_KIB=4096
truncate -s "${ESP_SIZE_KIB}KiB" "$ESP_IMG"
mkfs.vfat -F 12 -n "$VOLID" "$ESP_IMG" >/dev/null

mmd -i "$ESP_IMG" ::EFI ::EFI/BOOT
mcopy -i "$ESP_IMG" "$EFI_BIN" "::EFI/BOOT/BOOTAA64.EFI"

# Also drop a top-level marker file so ISO-mounters can tell what
# this disc is even when not booting.
mkdir -p "$STAGE/hyperion"
cp "$EFI_BIN" "$STAGE/hyperion/BOOTAA64.EFI"
cat > "$STAGE/README.TXT" <<'EOF'
Hyperion OS bootable ISO.

This is a hybrid ARM64 UEFI ISO. Boot it on:
  - QEMU virt + AAVMF UEFI firmware
  - Any UEFI ARM64 system (server, dev board with UEFI firmware)
  - From USB after writing with Rufus (DD mode), `dd`, or balenaEtcher

The UEFI firmware loads /EFI/BOOT/BOOTAA64.EFI from the embedded
EFI System Partition. That stub initialises the framebuffer via
the EFI Graphics Output Protocol and paints a Hyperion test
pattern. Kernel handover is wired up in a follow-up iteration.
EOF

echo "==> building hybrid ISO at $OUT"
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
echo "    size: $ISO_SIZE bytes ($((ISO_SIZE / 1024)) KiB)"
echo
echo "Boot it under QEMU + AAVMF:"
echo "    make run-iso"
echo
echo "Flash to USB:"
echo "    Linux:   sudo dd if=$OUT of=/dev/sdX bs=4M status=progress conv=fsync"
echo "    Windows: Rufus -> Select $OUT -> 'Write in DD Image mode'"
echo "    Mac:     sudo dd if=$OUT of=/dev/rdiskN bs=4m"
