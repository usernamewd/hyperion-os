# Hyperion OS top-level Makefile.
#
# Common targets:
#   make build     - build the kernel (release)
#   make debug     - build the kernel (dev profile)
#   make run       - boot under QEMU (serial-only)
#   make run-gfx   - boot under QEMU with a virtio-gpu display window
#   make clippy    - run clippy with -D warnings
#   make fmt       - format all crates
#   make doc       - build rustdoc for libos-api
#   make clean     - cargo clean

CARGO       ?= cargo
KERNEL_PKG  := hyperion-kernel
TARGET      := aarch64-unknown-none
PROFILE     ?= release
PROFILE_DIR := $(if $(filter release,$(PROFILE)),release,debug)
KERNEL_BIN  := target/$(TARGET)/$(PROFILE_DIR)/$(KERNEL_PKG)

QEMU        ?= qemu-system-aarch64
QEMU_BASE   := $(QEMU) -M virt -cpu cortex-a72 -smp 1 -m 512M -semihosting
QEMU_SERIAL := $(QEMU_BASE) -nographic
QEMU_GFX    := $(QEMU_BASE) -serial stdio -device virtio-gpu-pci

.PHONY: build debug run run-gfx clippy fmt fmt-check doc clean

build:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET) --release

debug:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET)

run: build
	$(QEMU_SERIAL) -kernel $(KERNEL_BIN)

run-gfx: build
	$(QEMU_GFX) -kernel $(KERNEL_BIN)

run-debug: debug
	$(QEMU_SERIAL) -kernel target/$(TARGET)/debug/$(KERNEL_PKG)

clippy:
	$(CARGO) clippy -p $(KERNEL_PKG) --target $(TARGET) --release -- -D warnings
	$(CARGO) clippy -p hyperion-os-api -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

doc:
	$(CARGO) doc -p hyperion-os-api --no-deps

clean:
	$(CARGO) clean
