# Hyperion OS top-level Makefile.
#
# Common targets:
#   make build      - build the kernel (release)
#   make debug      - build the kernel (dev profile)
#   make efi        - build the UEFI boot stub (.efi PE)
#   make esp        - assemble a FAT32 EFI System Partition tree
#   make iso        - assemble a hybrid ARM64 UEFI bootable ISO
#   make usb-img    - assemble a raw GPT disk image flashable via Rufus / dd
#   make run        - boot under QEMU (serial-only)
#   make run-gfx    - boot under QEMU with a virtio-gpu display window
#   make run-efi    - boot the EFI stub under QEMU + AAVMF UEFI firmware
#   make run-iso    - boot the ISO under QEMU + AAVMF (CD-ROM path)
#   make run-usb    - boot the raw USB image under QEMU + AAVMF
#   make clippy     - run clippy with -D warnings
#   make fmt        - format all crates
#   make doc        - build rustdoc for libos-api
#   make clean      - cargo clean

CARGO       ?= cargo
KERNEL_PKG  := hyperion-kernel
EFI_PKG     := hyperion-efi-stub
TARGET      := aarch64-unknown-none
EFI_TARGET  := aarch64-unknown-uefi
PROFILE     ?= release
PROFILE_DIR := $(if $(filter release,$(PROFILE)),release,debug)
KERNEL_BIN  := target/$(TARGET)/$(PROFILE_DIR)/$(KERNEL_PKG)
EFI_BIN     := target/$(EFI_TARGET)/$(PROFILE_DIR)/$(EFI_PKG).efi

QEMU        ?= qemu-system-aarch64
QEMU_BASE   := $(QEMU) -M virt -cpu cortex-a72 -smp 1 -m 512M -semihosting
QEMU_SERIAL := $(QEMU_BASE) -nographic
QEMU_GFX    := $(QEMU_BASE) -serial stdio -device virtio-gpu-pci

# UEFI firmware images shipped by qemu-efi-aarch64 (Ubuntu/Debian) or
# AAVMF (RedHat/Fedora). Override with AAVMF_CODE / AAVMF_VARS if you
# keep them somewhere else.
AAVMF_CODE  ?= /usr/share/AAVMF/AAVMF_CODE.fd
AAVMF_VARS  ?= /usr/share/AAVMF/AAVMF_VARS.fd

ESP_DIR     := target/esp
ESP_VARS    := target/AAVMF_VARS.fd
ISO_OUT     := target/hyperion.iso
USB_IMG     := target/hyperion-usb.img
ISO_VARS    := target/AAVMF_VARS_iso.fd
USB_VARS    := target/AAVMF_VARS_usb.fd

.PHONY: build debug efi esp iso usb-img run run-gfx run-efi run-iso run-usb \
        clippy fmt fmt-check doc clean

build:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET) --release

debug:
	$(CARGO) build -p $(KERNEL_PKG) --target $(TARGET)

efi:
	$(CARGO) build -p $(EFI_PKG) --target $(EFI_TARGET) --release

esp: efi
	mkdir -p $(ESP_DIR)/EFI/BOOT
	cp $(EFI_BIN) $(ESP_DIR)/EFI/BOOT/BOOTAA64.EFI

run: build
	$(QEMU_SERIAL) -kernel $(KERNEL_BIN)

run-gfx: build
	$(QEMU_GFX) -kernel $(KERNEL_BIN)

run-efi: esp
	cp $(AAVMF_VARS) $(ESP_VARS)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(AAVMF_CODE) \
	    -drive if=pflash,format=raw,file=$(ESP_VARS) \
	    -drive if=virtio,format=raw,file=fat:rw:$(ESP_DIR),readonly=off

iso:
	./scripts/build-iso.sh $(ISO_OUT)

usb-img:
	./scripts/build-usb-img.sh $(USB_IMG)

run-iso: iso
	cp $(AAVMF_VARS) $(ISO_VARS)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(AAVMF_CODE) \
	    -drive if=pflash,format=raw,file=$(ISO_VARS) \
	    -drive media=cdrom,file=$(ISO_OUT),if=none,id=cd0 \
	    -device virtio-scsi-pci -device scsi-cd,drive=cd0

run-usb: usb-img
	cp $(AAVMF_VARS) $(USB_VARS)
	$(QEMU_BASE) -display none -serial stdio -device ramfb \
	    -drive if=pflash,format=raw,readonly=on,file=$(AAVMF_CODE) \
	    -drive if=pflash,format=raw,file=$(USB_VARS) \
	    -drive if=virtio,format=raw,file=$(USB_IMG)

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
