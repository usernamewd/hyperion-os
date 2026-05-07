# Hyperion OS top-level Makefile.
#
# Hyperion targets two architectures: aarch64 (ARMv8) and x86_64
# (amd64). Choose between them with `ARCH=`:
#
#   make ARCH=aarch64 ...   (default)
#   make ARCH=x86_64 ...
#
# Common targets:
#   make build            - build the kernel for $(ARCH)
#   make debug            - build the kernel (dev profile) for $(ARCH)
#   make iso              - build bootable ISO(s) for $(ARCH)
#                           (aarch64: UEFI; x86_64: BIOS and UEFI)
#   make run              - boot under QEMU (serial-only, -kernel handoff)
#   make run-bios         - x86_64: boot ISO under QEMU SeaBIOS (legacy BIOS)
#   make run-uefi         - boot ISO under QEMU + UEFI (OVMF for x86_64,
#                           AAVMF for aarch64)
#   make run-gfx          - boot under QEMU with a graphical display window
#   make clippy           - run clippy with -D warnings (for $(ARCH))
#   make clippy-all       - run clippy for both arches
#   make build-all        - build the kernel for both arches
#   make fmt              - format all crates
#   make fmt-check        - check formatting without modifying
#   make doc              - build rustdoc for libos-api
#   make clean            - cargo clean

CARGO       ?= cargo
KERNEL_PKG  := hyperion-kernel
EFI_PKG     := hyperion-efi-stub
ARCH        ?= aarch64
PROFILE     ?= release
PROFILE_DIR := $(if $(filter release,$(PROFILE)),release,debug)

ifeq ($(ARCH),aarch64)
    TARGET      := aarch64-unknown-none
    EFI_TARGET  := aarch64-unknown-uefi
    QEMU        ?= qemu-system-aarch64
    QEMU_BASE   := $(QEMU) -M virt -cpu cortex-a72 -smp 1 -m 512M -semihosting
    QEMU_GFX    := $(QEMU_BASE) -serial stdio -device virtio-gpu-pci
    UEFI_CODE   ?= /usr/share/AAVMF/AAVMF_CODE.fd
    UEFI_VARS   ?= /usr/share/AAVMF/AAVMF_VARS.fd
    EFI_BOOT_FN := BOOTAA64.EFI
    UEFI_ISO_OUT := target/hyperion-aarch64-uefi.iso
    ISO_OUT     := $(UEFI_ISO_OUT)
    USB_IMG     := target/hyperion-usb.img
else ifeq ($(ARCH),x86_64)
    TARGET      := x86_64-unknown-none
    EFI_TARGET  := x86_64-unknown-uefi
    QEMU        ?= qemu-system-x86_64
    QEMU_BASE   := $(QEMU) -M q35 -cpu qemu64 -smp 1 -m 512M -no-reboot -no-shutdown
    QEMU_GFX    := $(QEMU_BASE) -serial stdio
    UEFI_CODE   ?= /usr/share/OVMF/OVMF_CODE.fd
    UEFI_VARS   ?= /usr/share/OVMF/OVMF_VARS.fd
    EFI_BOOT_FN := BOOTX64.EFI
    BIOS_ISO_OUT := target/hyperion-x86_64-bios.iso
    UEFI_ISO_OUT := target/hyperion-x86_64-uefi.iso
    ISO_OUT     := $(UEFI_ISO_OUT)
    USB_IMG     := target/hyperion-x86_64-usb.img
else
    $(error unsupported ARCH=$(ARCH); choose aarch64 or x86_64)
endif

KERNEL_BIN  := target/$(TARGET)/$(PROFILE_DIR)/$(KERNEL_PKG)
EFI_BIN     := target/$(EFI_TARGET)/$(PROFILE_DIR)/$(EFI_PKG).efi
EFI_KERNEL_ENV := HYPERION_KERNEL_ELF=$(abspath $(KERNEL_BIN))
QEMU_SERIAL := $(QEMU_BASE) -display none -serial stdio

# Backwards-compat aliases (older docs referred to AAVMF_*).
AAVMF_CODE  ?= $(UEFI_CODE)
AAVMF_VARS  ?= $(UEFI_VARS)

ESP_DIR     := target/esp-$(ARCH)
ESP_VARS    := target/$(ARCH)-VARS.fd
ISO_VARS    := target/$(ARCH)-VARS_iso.fd
USB_VARS    := target/$(ARCH)-VARS_usb.fd

.PHONY: build debug build-all efi esp iso iso-bios iso-uefi usb-img run run-gfx run-efi \
        run-iso run-usb run-bios run-uefi run-debug \
        clippy clippy-all clippy-arch clippy-host fmt fmt-check doc clean

build:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET) --release

debug:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET)

build-all:
	$(CARGO) build -p $(KERNEL_PKG) --target aarch64-unknown-none --release
	$(CARGO) build -p $(KERNEL_PKG) --target x86_64-unknown-none --release

efi:
	CARGO_PROFILE_RELEASE_DEBUG=false CARGO_PROFILE_RELEASE_STRIP=symbols $(CARGO) build -p $(KERNEL_PKG) --target $(TARGET) --release
	$(EFI_KERNEL_ENV) $(CARGO) build -p $(EFI_PKG) --target $(EFI_TARGET) --release

esp: efi
	mkdir -p $(ESP_DIR)/EFI/BOOT
	cp $(EFI_BIN) $(ESP_DIR)/EFI/BOOT/$(EFI_BOOT_FN)

run: build
ifeq ($(ARCH),aarch64)
	$(QEMU_BASE) -nographic -kernel $(KERNEL_BIN)
else
	$(QEMU_SERIAL) -kernel $(KERNEL_BIN)
endif

run-gfx: build
	$(QEMU_GFX) -kernel $(KERNEL_BIN)

run-debug: debug
ifeq ($(ARCH),aarch64)
	$(QEMU_BASE) -nographic -kernel target/$(TARGET)/debug/$(KERNEL_PKG)
else
	$(QEMU_SERIAL) -kernel target/$(TARGET)/debug/$(KERNEL_PKG)
endif

run-efi: esp
	cp $(UEFI_VARS) $(ESP_VARS)
ifeq ($(ARCH),aarch64)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(ESP_VARS) \
	    -drive if=virtio,format=raw,file=fat:rw:$(ESP_DIR),readonly=off
else
	$(QEMU_SERIAL) \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(ESP_VARS) \
	    -drive if=virtio,format=raw,file=fat:rw:$(ESP_DIR),readonly=off
endif

iso:
ifeq ($(ARCH),aarch64)
	$(MAKE) ARCH=$(ARCH) iso-uefi
else
	$(MAKE) ARCH=$(ARCH) iso-bios
	$(MAKE) ARCH=$(ARCH) iso-uefi
endif

iso-bios:
ifeq ($(ARCH),x86_64)
	./scripts/build-iso-x86_64.sh bios $(BIOS_ISO_OUT)
else
	@echo "BIOS ISO is x86_64-only."
	exit 1
endif

iso-uefi:
ifeq ($(ARCH),aarch64)
	./scripts/build-iso.sh $(UEFI_ISO_OUT)
else
	./scripts/build-iso-x86_64.sh uefi $(UEFI_ISO_OUT)
endif

usb-img:
ifeq ($(ARCH),aarch64)
	./scripts/build-usb-img.sh $(USB_IMG)
else
	./scripts/build-usb-img-x86_64.sh $(USB_IMG)
endif

run-bios: iso-bios
ifeq ($(ARCH),x86_64)
	$(QEMU_SERIAL) -cdrom $(BIOS_ISO_OUT)
else
	@echo "BIOS path is x86_64-only."
	exit 1
endif

run-uefi: iso-uefi
	cp $(UEFI_VARS) $(ISO_VARS)
ifeq ($(ARCH),aarch64)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(ISO_VARS) \
	    -drive media=cdrom,file=$(UEFI_ISO_OUT),if=none,id=cd0 \
	    -device virtio-scsi-pci -device scsi-cd,drive=cd0
else
	$(QEMU_SERIAL) \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(ISO_VARS) \
	    -cdrom $(UEFI_ISO_OUT)
endif

run-iso: run-uefi

run-usb: usb-img
	cp $(UEFI_VARS) $(USB_VARS)
ifeq ($(ARCH),aarch64)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(USB_VARS) \
	    -drive if=virtio,format=raw,file=$(USB_IMG)
else
	$(QEMU_SERIAL) \
	    -drive if=pflash,format=raw,readonly=on,file=$(UEFI_CODE) \
	    -drive if=pflash,format=raw,file=$(USB_VARS) \
	    -drive if=virtio,format=raw,file=$(USB_IMG)
endif

clippy:
	$(CARGO) clippy -p $(KERNEL_PKG) --target $(TARGET) --release -- -D warnings
	$(CARGO) clippy -p hyperion-os-api -- -D warnings

clippy-arch: clippy

clippy-host:
	$(CARGO) clippy -p hyperion-os-api -- -D warnings

clippy-all: clippy-host
	$(CARGO) clippy -p $(KERNEL_PKG) --target aarch64-unknown-none --release -- -D warnings
	$(CARGO) clippy -p $(KERNEL_PKG) --target x86_64-unknown-none --release -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

doc:
	$(CARGO) doc -p hyperion-os-api --no-deps

clean:
	$(CARGO) clean
