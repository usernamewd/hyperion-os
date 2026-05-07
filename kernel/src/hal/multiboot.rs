//! Multiboot2 information parser.
//!
//! Sister to [`super::dtb`]. The aarch64 boot path receives a flattened
//! device tree on `x0`; the x86_64 boot path receives a Multiboot2
//! information block in `RBX` (set up by GRUB-PC for BIOS, GRUB-EFI
//! for UEFI). This module decodes the relevant tags and lowers them
//! into a [`BootInfo`] instance the rest of the kernel consumes.
//!
//! Only the tags Hyperion actually uses are decoded:
//!
//! * basic memory info (legacy `mem_lower` / `mem_upper`),
//! * memory map,
//! * framebuffer (RGB/BGR colour mask),
//! * RSDP descriptor (ACPI 1.0 / 2.0+),
//! * EFI 64-bit image handle / system table.
//!
//! Anything else is ignored — and a malformed blob falls back to
//! [`BootInfo::qemu_q35_fallback`].

use super::boot_info::{
    BootInfo, ConsoleKind, ConsoleSpec, FramebufferInfo, IntcKind, IntcSpec, MemoryMap,
    MemoryRegion, PixelFormat, RegSpec,
};

const MB2_MAGIC: u64 = 0x36d76289;

const TAG_END: u32 = 0;
const TAG_CMDLINE: u32 = 1;
const TAG_BOOTLOADER_NAME: u32 = 2;
const TAG_BASIC_MEM_INFO: u32 = 4;
const TAG_MEMORY_MAP: u32 = 6;
const TAG_FRAMEBUFFER_INFO: u32 = 8;
const TAG_EFI64_SYSTAB: u32 = 12;
const TAG_RSDP_OLD: u32 = 14;
const TAG_RSDP_NEW: u32 = 15;
const TAG_EFI64_IMAGE: u32 = 20;

const MEM_TYPE_AVAILABLE: u32 = 1;

#[repr(C)]
struct TagHeader {
    typ: u32,
    size: u32,
}

#[repr(C)]
struct MemMapEntry {
    base_addr: u64,
    length: u64,
    typ: u32,
    _reserved: u32,
}

#[repr(C)]
struct FbTag {
    typ: u32,
    size: u32,
    addr: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    fb_type: u8,
    _reserved: u16,
    // followed by colour info, see fb_color_info
}

#[repr(C)]
struct FbRgb {
    red_field_position: u8,
    red_mask_size: u8,
    green_field_position: u8,
    green_mask_size: u8,
    blue_field_position: u8,
    blue_mask_size: u8,
}

/// Walk the Multiboot2 info structure pointed to by `mbi_ptr` (in 64-bit
/// flat physical address form) and return a populated [`BootInfo`].
///
/// If the magic is wrong, the size header is bogus, or no usable tags
/// are present, returns [`BootInfo::qemu_q35_fallback`].
///
/// # Safety
/// `mbi_ptr` must be either zero (we'll fall back) or the address of a
/// well-formed Multiboot2 info block. We only read; nothing in `bi` is
/// retained beyond the call.
pub unsafe fn parse_or_fallback(mbi_ptr: u64, magic: u64) -> BootInfo {
    let mut bi = BootInfo::qemu_q35_fallback();
    bi.fw_table_addr = mbi_ptr;

    // Always wire the LAPIC + IOAPIC into the IntcSpec on x86_64; even
    // if Multiboot2 doesn't tell us about them they sit at the reset
    // base everywhere.
    bi.intc = IntcSpec {
        kind: IntcKind::Apic,
        primary: RegSpec::new(0xFEE0_0000, 0x1000),
        secondary: RegSpec::new(0xFEC0_0000, 0x1000),
    };

    if magic != MB2_MAGIC || mbi_ptr == 0 {
        return bi;
    }

    // SAFETY: caller asserted `mbi_ptr` is a valid Multiboot2 info ptr.
    let total_size = unsafe { core::ptr::read_volatile(mbi_ptr as *const u32) };
    if !(16..=16 * 1024 * 1024).contains(&total_size) {
        return bi;
    }

    let end = mbi_ptr.saturating_add(total_size as u64);
    // First tag follows the 8-byte header (total_size + reserved).
    let mut cur = mbi_ptr + 8;
    let mut have_real_memory = false;

    while cur + (core::mem::size_of::<TagHeader>() as u64) <= end {
        // SAFETY: `cur` is bounded by `end`, which is bounded by total_size.
        let hdr = unsafe { &*(cur as *const TagHeader) };
        if hdr.typ == TAG_END {
            break;
        }
        if hdr.size < 8 {
            break;
        }

        match hdr.typ {
            TAG_MEMORY_MAP => {
                // Body: u32 entry_size + u32 entry_version + entries.
                let body = cur + (core::mem::size_of::<TagHeader>() as u64);
                let entry_size = unsafe { core::ptr::read_volatile(body as *const u32) } as u64;
                let entries_start = body + 8;
                let entries_end = cur + (hdr.size as u64);
                let mut p = entries_start;
                let mut new_map = MemoryMap::empty();
                while p + entry_size <= entries_end && entry_size != 0 {
                    let e = unsafe { &*(p as *const MemMapEntry) };
                    if e.typ == MEM_TYPE_AVAILABLE && e.length != 0 {
                        new_map.push(MemoryRegion::new(e.base_addr, e.length));
                    }
                    p += entry_size;
                }
                if new_map.total_bytes() != 0 {
                    bi.memory = new_map;
                    have_real_memory = true;
                }
            }
            TAG_BASIC_MEM_INFO => {
                if !have_real_memory {
                    let body = cur + (core::mem::size_of::<TagHeader>() as u64);
                    let mem_lower = unsafe { core::ptr::read_volatile(body as *const u32) } as u64;
                    let mem_upper =
                        unsafe { core::ptr::read_volatile((body + 4) as *const u32) } as u64;
                    let mut new_map = MemoryMap::empty();
                    if mem_lower != 0 {
                        new_map.push(MemoryRegion::new(0, mem_lower * 1024));
                    }
                    if mem_upper != 0 {
                        // mem_upper is in KiB starting at 1 MiB.
                        new_map.push(MemoryRegion::new(0x10_0000, mem_upper * 1024));
                    }
                    if new_map.total_bytes() != 0 {
                        bi.memory = new_map;
                    }
                }
            }
            TAG_FRAMEBUFFER_INFO => {
                if (hdr.size as usize) >= core::mem::size_of::<FbTag>() {
                    let fb = unsafe { &*(cur as *const FbTag) };
                    if fb.fb_type == 1 && fb.bpp == 32 {
                        // type 1 = direct RGB, packed bits.
                        let rgb = unsafe {
                            &*((cur + core::mem::size_of::<FbTag>() as u64) as *const FbRgb)
                        };
                        let format = if rgb.red_field_position >= 16 {
                            PixelFormat::Bgra8888
                        } else {
                            PixelFormat::Rgba8888
                        };
                        bi.framebuffer = Some(FramebufferInfo {
                            base: fb.addr,
                            width: fb.width,
                            height: fb.height,
                            stride_bytes: fb.pitch,
                            bpp: fb.bpp as u32,
                            format,
                        });
                    }
                }
            }
            TAG_RSDP_OLD | TAG_RSDP_NEW => {
                // Stash the RSDP physical address into fw_table_addr if
                // we don't already have a higher-priority pointer.
                let body = cur + (core::mem::size_of::<TagHeader>() as u64);
                bi.fw_table_addr = body;
            }
            TAG_EFI64_SYSTAB => {
                // The EFI system table address is what the EFI runtime
                // services are reached through. For now we just record
                // its presence by setting fw_table_addr if it's not
                // already aimed at an RSDP.
                if bi.fw_table_addr == mbi_ptr {
                    let body = cur + (core::mem::size_of::<TagHeader>() as u64);
                    bi.fw_table_addr = unsafe { core::ptr::read_volatile(body as *const u64) };
                }
            }
            TAG_CMDLINE | TAG_BOOTLOADER_NAME | TAG_EFI64_IMAGE => {
                // Not consumed for boot decisions yet, but valid tags.
            }
            _ => {}
        }

        // Tags are 8-byte aligned.
        cur += (hdr.size as u64 + 7) & !7;
    }

    // Console: NS16550 at COM1 always exists on PC platforms; the
    // fallback already filled this in. If a real serial discovery via
    // ACPI gets added later, override here.
    if matches!(
        bi.console.kind,
        ConsoleKind::Pl011 | ConsoleKind::BcmMiniUart
    ) {
        bi.console = ConsoleSpec {
            kind: ConsoleKind::Ns16550,
            regs: RegSpec::new(0x3F8, 8),
            clock_hz: 1_843_200,
        };
    }

    bi
}
