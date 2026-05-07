#!/usr/bin/env bash
# Boot Hyperion under QEMU. Usage: scripts/run-qemu.sh [release|debug] [--gfx]
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-release}"
GFX=""
if [[ "${2:-}" == "--gfx" ]]; then
    GFX="--gfx"
fi

PROFILE_FLAG=""
if [[ "$PROFILE" == "release" ]]; then
    PROFILE_FLAG="--release"
fi

cargo build -p hyperion-kernel --target aarch64-unknown-none $PROFILE_FLAG

KERNEL="target/aarch64-unknown-none/${PROFILE}/hyperion-kernel"

if [[ -n "$GFX" ]]; then
    exec qemu-system-aarch64 -M virt -cpu cortex-a72 -smp 1 -m 512M \
        -semihosting -serial stdio -device virtio-gpu-pci -kernel "$KERNEL"
else
    exec qemu-system-aarch64 -M virt -cpu cortex-a72 -smp 1 -m 512M \
        -semihosting -nographic -kernel "$KERNEL"
fi
