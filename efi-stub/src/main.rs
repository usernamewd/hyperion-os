//! Hyperion OS UEFI boot stub.
//!
//! This is a tiny `aarch64-unknown-uefi` PE that runs as an EFI
//! application. Its job is to discover the firmware-provided graphics
//! output (GOP) and memory map, then hand control to the Hyperion
//! kernel proper.
//!
//! The stub:
//!
//! 1. Locates the **Graphics Output Protocol** (GOP) via Boot Services
//!    and reads the active mode's framebuffer base / resolution /
//!    stride / pixel format.
//! 2. Loads the embedded `aarch64-unknown-none` kernel ELF into its
//!    fixed physical segments.
//! 3. Builds a compact handoff block from GOP and the UEFI memory map.
//! 4. Calls `ExitBootServices`, disables firmware MMU/cache state, and
//!    jumps to the kernel entry with the handoff pointer in `x0`.
//!
//! The stub is deliberately self-contained — no `uefi` crate
//! dependency, just hand-rolled UEFI types — so it stays small,
//! reviewable, and free of surprise transitive deps.

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::mem;
use core::panic::PanicInfo;
use core::ptr;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt()
}

fn halt() -> ! {
    loop {
        // SAFETY: WFE is always safe; just stalls the core.
        unsafe { core::arch::asm!("wfe", options(nomem, nostack)) };
    }
}

// ---------- Minimal UEFI ABI ----------

#[allow(non_camel_case_types)]
type EFI_STATUS = usize;
#[allow(non_camel_case_types)]
type EFI_HANDLE = *mut c_void;

const EFI_SUCCESS: EFI_STATUS = 0;
const EFI_PAGE_SIZE: u64 = 4096;
const EFI_ALLOCATE_ADDRESS: u32 = 2;
const EFI_LOADER_DATA: u32 = 2;
const EFI_CONVENTIONAL_MEMORY: u32 = 7;
const MEMORY_MAP_BUF_SIZE: usize = 64 * 1024;
const MAX_MEMORY_REGIONS: usize = 8;
const UEFI_HANDOFF_MAGIC: u64 = 0x4859_5055_4546_4948;
const UEFI_HANDOFF_VERSION: u32 = 1;
const PT_LOAD: u32 = 1;

static KERNEL_ELF: &[u8] = include_bytes!(env!("HYPERION_KERNEL_ELF"));
static mut MEMORY_MAP_BUF: [u8; MEMORY_MAP_BUF_SIZE] = [0; MEMORY_MAP_BUF_SIZE];

// Status codes use the high bit on 64-bit systems; we only need to
// distinguish "ok" from "anything else".

#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct EfiGuid(u32, u16, u16, [u8; 8]);

const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid(
    0x9042a9de,
    0x23dc,
    0x4a38,
    [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
);

#[repr(C)]
struct EfiSimpleTextOutputProtocol {
    reset: extern "efiapi" fn(*mut EfiSimpleTextOutputProtocol, bool) -> EFI_STATUS,
    output_string: extern "efiapi" fn(*mut EfiSimpleTextOutputProtocol, *const u16) -> EFI_STATUS,
    // [snip: the rest of the protocol vtable; we only need OutputString]
}

#[repr(C)]
struct EfiBootServices {
    hdr: EfiTableHeader,

    // Task Priority
    raise_tpl: *const c_void,
    restore_tpl: *const c_void,

    // Memory
    allocate_pages: extern "efiapi" fn(u32, u32, usize, *mut u64) -> EFI_STATUS,
    free_pages: *const c_void,
    get_memory_map: extern "efiapi" fn(
        *mut usize,  // mmap_size in/out
        *mut c_void, // mmap buffer
        *mut usize,  // map_key out
        *mut usize,  // descriptor_size out
        *mut u32,    // descriptor_version out
    ) -> EFI_STATUS,
    allocate_pool: *const c_void,
    free_pool: *const c_void,

    // Event/Timer
    create_event: *const c_void,
    set_timer: *const c_void,
    wait_for_event: *const c_void,
    signal_event: *const c_void,
    close_event: *const c_void,
    check_event: *const c_void,

    // Protocol Handler
    install_protocol_interface: *const c_void,
    reinstall_protocol_interface: *const c_void,
    uninstall_protocol_interface: *const c_void,
    handle_protocol: *const c_void,
    reserved: *const c_void,
    register_protocol_notify: *const c_void,
    locate_handle: *const c_void,
    locate_device_path: *const c_void,
    install_configuration_table: *const c_void,

    // Image
    load_image: *const c_void,
    start_image: *const c_void,
    exit: *const c_void,
    unload_image: *const c_void,
    exit_boot_services: extern "efiapi" fn(EFI_HANDLE, usize) -> EFI_STATUS,

    // Misc
    get_next_monotonic_count: *const c_void,
    stall: *const c_void,
    set_watchdog_timer: *const c_void,

    // DriverSupport
    connect_controller: *const c_void,
    disconnect_controller: *const c_void,

    // Open/Close Protocol
    open_protocol: *const c_void,
    close_protocol: *const c_void,
    open_protocol_information: *const c_void,

    // Library
    protocols_per_handle: *const c_void,
    locate_handle_buffer: *const c_void,
    locate_protocol: extern "efiapi" fn(
        *const EfiGuid,
        *mut c_void, // registration (unused)
        *mut *mut c_void,
    ) -> EFI_STATUS,
    install_multiple_protocol_interfaces: *const c_void,
    uninstall_multiple_protocol_interfaces: *const c_void,

    // 32-bit CRC Services
    calculate_crc32: *const c_void,

    // Misc
    copy_mem: *const c_void,
    set_mem: *const c_void,
    create_event_ex: *const c_void,
}

#[repr(C)]
struct EfiSystemTable {
    hdr: EfiTableHeader,
    firmware_vendor: *const u16,
    firmware_revision: u32,

    console_in_handle: EFI_HANDLE,
    con_in: *const c_void,

    console_out_handle: EFI_HANDLE,
    con_out: *mut EfiSimpleTextOutputProtocol,

    standard_error_handle: EFI_HANDLE,
    std_err: *mut EfiSimpleTextOutputProtocol,

    runtime_services: *const c_void,
    boot_services: *mut EfiBootServices,

    number_of_table_entries: usize,
    configuration_table: *const c_void,
}

// ---------- GOP types ----------

#[repr(u32)]
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
enum EfiGraphicsPixelFormat {
    PixelRedGreenBlueReserved8BitPerColor = 0,
    PixelBlueGreenRedReserved8BitPerColor = 1,
    PixelBitMask = 2,
    PixelBltOnly = 3,
    PixelFormatMax = 4,
}

#[repr(C)]
#[allow(dead_code)]
struct EfiPixelBitmask {
    red: u32,
    green: u32,
    blue: u32,
    reserved: u32,
}

#[repr(C)]
struct EfiGraphicsOutputModeInformation {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: u32,
    pixel_information: EfiPixelBitmask,
    pixels_per_scan_line: u32,
}

#[repr(C)]
struct EfiGraphicsOutputProtocolMode {
    max_mode: u32,
    mode: u32,
    info: *const EfiGraphicsOutputModeInformation,
    size_of_info: usize,
    framebuffer_base: u64,
    framebuffer_size: usize,
}

#[repr(C)]
struct EfiGraphicsOutputProtocol {
    query_mode: *const c_void,
    set_mode: *const c_void,
    blt: *const c_void,
    mode: *mut EfiGraphicsOutputProtocolMode,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiMemoryDescriptor {
    typ: u32,
    _pad: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HandoffMemoryRegion {
    base: u64,
    size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiHandoff {
    magic: u64,
    version: u32,
    _reserved0: u32,
    console_kind: u32,
    console_base: u64,
    console_size: u64,
    console_clock_hz: u32,
    intc_kind: u32,
    intc_primary_base: u64,
    intc_primary_size: u64,
    intc_secondary_base: u64,
    intc_secondary_size: u64,
    timer_freq_hz: u32,
    memory_len: u32,
    memory: [HandoffMemoryRegion; MAX_MEMORY_REGIONS],
    framebuffer_present: u32,
    framebuffer_base: u64,
    framebuffer_size: u64,
    framebuffer_width: u32,
    framebuffer_height: u32,
    framebuffer_stride_bytes: u32,
    framebuffer_bpp: u32,
    framebuffer_format: u32,
    fw_table_addr: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

struct MemoryMapSnapshot {
    ptr: *mut u8,
    size: usize,
    key: usize,
    desc_size: usize,
}

// ---------- Helpers ----------

/// Convert an ASCII byte slice to a stack-allocated UCS-2 array,
/// NUL-terminated, ready for `output_string`. Capped at 256 chars.
fn ascii_to_ucs2(s: &str, out: &mut [u16; 256]) -> usize {
    let mut i = 0;
    for b in s.bytes() {
        if i + 1 >= out.len() {
            break;
        }
        out[i] = b as u16;
        i += 1;
    }
    out[i] = 0;
    i
}

fn print(con_out: *mut EfiSimpleTextOutputProtocol, s: &str) {
    let mut buf = [0u16; 256];
    ascii_to_ucs2(s, &mut buf);
    // SAFETY: con_out is provided by firmware and lives for the
    // duration of boot services. The buffer is NUL-terminated.
    unsafe {
        let f = (*con_out).output_string;
        let _ = f(con_out, buf.as_ptr());
    }
}

/// Print a small unsigned integer in decimal.
fn print_u32(con_out: *mut EfiSimpleTextOutputProtocol, mut n: u32) {
    if n == 0 {
        print(con_out, "0");
        return;
    }
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let s = core::str::from_utf8(&buf[i..]).unwrap_or("?");
    print(con_out, s);
}

/// Print a u64 in lower-case hex with `0x` prefix.
fn print_u64_hex(con_out: *mut EfiSimpleTextOutputProtocol, n: u64) {
    print(con_out, "0x");
    let mut buf = [0u8; 16];
    for (i, slot) in buf.iter_mut().enumerate() {
        let nyb = ((n >> ((15 - i) * 4)) & 0xf) as u8;
        *slot = if nyb < 10 {
            b'0' + nyb
        } else {
            b'a' + (nyb - 10)
        };
    }
    let s = core::str::from_utf8(&buf).unwrap_or("?");
    print(con_out, s);
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    value.checked_add(align - 1).map(|v| align_down(v, align))
}

fn read_at<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(mem::size_of::<T>())?;
    if end > bytes.len() {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().add(offset) as *const T) })
}

fn load_kernel_elf(bs: *mut EfiBootServices) -> Option<u64> {
    let ehdr: Elf64Ehdr = read_at(KERNEL_ELF, 0)?;
    if &ehdr.e_ident[0..4] != b"\x7fELF"
        || ehdr.e_ident[4] != 2
        || ehdr.e_ident[5] != 1
        || ehdr.e_machine != 0xb7
        || ehdr.e_phentsize as usize != mem::size_of::<Elf64Phdr>()
    {
        return None;
    }

    let mut load_min = u64::MAX;
    let mut load_max = 0u64;
    for i in 0..ehdr.e_phnum as usize {
        let off = (ehdr.e_phoff as usize).checked_add(i.checked_mul(ehdr.e_phentsize as usize)?)?;
        let phdr: Elf64Phdr = read_at(KERNEL_ELF, off)?;
        if phdr.p_type != PT_LOAD || phdr.p_memsz == 0 {
            continue;
        }
        let start = align_down(phdr.p_paddr, EFI_PAGE_SIZE);
        let end = align_up(phdr.p_paddr.checked_add(phdr.p_memsz)?, EFI_PAGE_SIZE)?;
        load_min = load_min.min(start);
        load_max = load_max.max(end);
    }
    if load_min == u64::MAX || load_max <= load_min {
        return None;
    }

    let pages = ((load_max - load_min) / EFI_PAGE_SIZE) as usize;
    let mut addr = load_min;
    let status =
        unsafe { ((*bs).allocate_pages)(EFI_ALLOCATE_ADDRESS, EFI_LOADER_DATA, pages, &mut addr) };
    if status != EFI_SUCCESS || addr != load_min {
        return None;
    }

    let len = (load_max - load_min) as usize;
    unsafe { ptr::write_bytes(load_min as *mut u8, 0, len) };
    for i in 0..ehdr.e_phnum as usize {
        let off = (ehdr.e_phoff as usize).checked_add(i.checked_mul(ehdr.e_phentsize as usize)?)?;
        let phdr: Elf64Phdr = read_at(KERNEL_ELF, off)?;
        if phdr.p_type != PT_LOAD || phdr.p_filesz == 0 {
            continue;
        }
        let file_end = phdr.p_offset.checked_add(phdr.p_filesz)? as usize;
        if file_end > KERNEL_ELF.len() {
            return None;
        }
        unsafe {
            ptr::copy_nonoverlapping(
                KERNEL_ELF.as_ptr().add(phdr.p_offset as usize),
                phdr.p_paddr as *mut u8,
                phdr.p_filesz as usize,
            );
        }
    }
    Some(ehdr.e_entry)
}

fn get_memory_map(bs: *mut EfiBootServices) -> Option<MemoryMapSnapshot> {
    let ptr = ptr::addr_of_mut!(MEMORY_MAP_BUF).cast::<u8>();
    let mut size = MEMORY_MAP_BUF_SIZE;
    let mut key = 0usize;
    let mut desc_size = 0usize;
    let mut desc_version = 0u32;
    let status = unsafe {
        ((*bs).get_memory_map)(
            &mut size,
            ptr.cast::<c_void>(),
            &mut key,
            &mut desc_size,
            &mut desc_version,
        )
    };
    if status != EFI_SUCCESS || desc_size < mem::size_of::<EfiMemoryDescriptor>() {
        return None;
    }
    Some(MemoryMapSnapshot {
        ptr,
        size,
        key,
        desc_size,
    })
}

fn push_memory_region(handoff: &mut EfiHandoff, base: u64, size: u64) {
    if size == 0 {
        return;
    }
    let len = handoff.memory_len as usize;
    if len > 0 {
        let prev = &mut handoff.memory[len - 1];
        if prev.base.saturating_add(prev.size) == base {
            prev.size = prev.size.saturating_add(size);
            return;
        }
    }
    if len < MAX_MEMORY_REGIONS {
        handoff.memory[len] = HandoffMemoryRegion { base, size };
        handoff.memory_len += 1;
    }
}

fn build_handoff(map: &MemoryMapSnapshot, mode: *mut EfiGraphicsOutputProtocolMode) -> EfiHandoff {
    let (fb_base, fb_size, w, h, stride, pf) = unsafe {
        let m = &*mode;
        let info = &*m.info;
        (
            m.framebuffer_base,
            m.framebuffer_size as u64,
            info.horizontal_resolution,
            info.vertical_resolution,
            info.pixels_per_scan_line.saturating_mul(4),
            info.pixel_format,
        )
    };
    let framebuffer_format =
        if pf == EfiGraphicsPixelFormat::PixelRedGreenBlueReserved8BitPerColor as u32 {
            1
        } else {
            0
        };
    let mut handoff = EfiHandoff {
        magic: UEFI_HANDOFF_MAGIC,
        version: UEFI_HANDOFF_VERSION,
        _reserved0: 0,
        console_kind: 0,
        console_base: 0x0900_0000,
        console_size: 0x1000,
        console_clock_hz: 24_000_000,
        intc_kind: 0,
        intc_primary_base: 0x0800_0000,
        intc_primary_size: 0x10000,
        intc_secondary_base: 0x0801_0000,
        intc_secondary_size: 0x10000,
        timer_freq_hz: 0,
        memory_len: 0,
        memory: [HandoffMemoryRegion { base: 0, size: 0 }; MAX_MEMORY_REGIONS],
        framebuffer_present: 1,
        framebuffer_base: fb_base,
        framebuffer_size: fb_size,
        framebuffer_width: w,
        framebuffer_height: h,
        framebuffer_stride_bytes: stride,
        framebuffer_bpp: 32,
        framebuffer_format,
        fw_table_addr: 0,
    };

    let mut off = 0usize;
    while off + mem::size_of::<EfiMemoryDescriptor>() <= map.size {
        let desc = unsafe { ptr::read_unaligned(map.ptr.add(off) as *const EfiMemoryDescriptor) };
        if desc.typ == EFI_CONVENTIONAL_MEMORY {
            push_memory_region(
                &mut handoff,
                desc.physical_start,
                desc.number_of_pages.saturating_mul(EFI_PAGE_SIZE),
            );
        }
        off = off.saturating_add(map.desc_size);
    }
    if handoff.memory_len == 0 {
        push_memory_region(&mut handoff, 0x4000_0000, 256 * 1024 * 1024);
    }
    handoff
}

fn exit_boot_services(
    image: EFI_HANDLE,
    bs: *mut EfiBootServices,
    con_out: *mut EfiSimpleTextOutputProtocol,
    mode: *mut EfiGraphicsOutputProtocolMode,
) -> EfiHandoff {
    for _ in 0..3 {
        let map = match get_memory_map(bs) {
            Some(map) => map,
            None => {
                print(con_out, "GetMemoryMap failed; halting.\r\n");
                halt();
            }
        };
        let handoff = build_handoff(&map, mode);
        let status = unsafe { ((*bs).exit_boot_services)(image, map.key) };
        if status == EFI_SUCCESS {
            return handoff;
        }
    }
    print(con_out, "ExitBootServices failed; halting.\r\n");
    halt();
}

fn prepare_cpu_for_kernel() {
    unsafe {
        core::arch::asm!(
            "msr daifset, #0xf",
            "dsb sy",
            "mrs x9, sctlr_el1",
            "bic x9, x9, #1",
            "bic x9, x9, #(1 << 2)",
            "bic x9, x9, #(1 << 12)",
            "msr sctlr_el1, x9",
            "isb",
            "tlbi vmalle1",
            "dsb sy",
            "isb",
            out("x9") _,
            options(nostack, preserves_flags)
        );
    }
}

fn jump_to_kernel(entry: u64, handoff: &EfiHandoff) -> ! {
    prepare_cpu_for_kernel();
    let kernel_entry: extern "C" fn(u64) -> ! = unsafe { mem::transmute(entry as usize) };
    kernel_entry(handoff as *const EfiHandoff as u64)
}

// ---------- GOP test pattern ----------

fn paint_pattern(mode: *mut EfiGraphicsOutputProtocolMode) {
    // SAFETY: firmware-published GOP mode struct; valid for the
    // lifetime of boot services.
    let (base, fb_size, w, h, stride, bgrx) = unsafe {
        let m = &*mode;
        let info = &*m.info;
        let bgrx = info.pixel_format
            == EfiGraphicsPixelFormat::PixelBlueGreenRedReserved8BitPerColor as u32;
        (
            m.framebuffer_base,
            m.framebuffer_size,
            info.horizontal_resolution,
            info.vertical_resolution,
            info.pixels_per_scan_line as usize,
            bgrx,
        )
    };
    if base == 0 || fb_size == 0 || w == 0 || h == 0 {
        return;
    }
    // Encode rgba -> 32-bit pixel for the active GOP format.
    let encode = |r: u8, g: u8, b: u8| -> u32 {
        if bgrx {
            (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xff << 24)
        } else {
            (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (0xff << 24)
        }
    };
    // Hyperion stripes: dark navy / cyan / magenta.
    let stripe_h = (h / 3).max(1);
    let bg_a = encode(0x10, 0x18, 0x24);
    let bg_b = encode(0x14, 0x6a, 0xa6);
    let bg_c = encode(0xa6, 0x4d, 0xc4);
    // SAFETY: framebuffer region was published by firmware; we are the
    // sole writer until ExitBootServices.
    let fb = base as *mut u32;
    for y in 0..h as usize {
        let band = if (y as u32) < stripe_h {
            bg_a
        } else if (y as u32) < 2 * stripe_h {
            bg_b
        } else {
            bg_c
        };
        for x in 0..w as usize {
            let off = y * stride + x;
            unsafe { ptr::write_volatile(fb.add(off), band) };
        }
    }
    // Centred white square (~10% of the smaller dimension).
    let side = (w.min(h) / 10).max(8);
    let cx = w / 2 - side / 2;
    let cy = h / 2 - side / 2;
    let white = encode(0xff, 0xff, 0xff);
    for y in cy..cy + side {
        for x in cx..cx + side {
            let off = (y as usize) * stride + (x as usize);
            // SAFETY: bounds-checked above (cx+side <= w, cy+side <= h).
            unsafe { ptr::write_volatile(fb.add(off), white) };
        }
    }
}

// ---------- Entry ----------

#[no_mangle]
extern "efiapi" fn efi_main(image: EFI_HANDLE, system_table: *mut EfiSystemTable) -> EFI_STATUS {
    // SAFETY: firmware guarantees a valid system table on entry.
    let st = unsafe { &mut *system_table };
    let con_out = st.con_out;
    let bs = st.boot_services;

    print(con_out, "\r\nHyperion EFI stub starting...\r\n");

    // ---- Locate GOP ----
    let mut gop_ptr: *mut c_void = ptr::null_mut();
    // SAFETY: LocateProtocol is a standard UEFI service call.
    let s = unsafe {
        ((*bs).locate_protocol)(
            &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
            ptr::null_mut(),
            &mut gop_ptr,
        )
    };
    if s != EFI_SUCCESS || gop_ptr.is_null() {
        print(con_out, "GOP not available; halting.\r\n");
        halt();
    }
    let gop = gop_ptr as *mut EfiGraphicsOutputProtocol;
    // SAFETY: GOP pointer was just validated.
    let mode = unsafe { (*gop).mode };
    // SAFETY: as above.
    let (fb_base, fb_size, w, h, stride, pf) = unsafe {
        let m = &*mode;
        let info = &*m.info;
        (
            m.framebuffer_base,
            m.framebuffer_size,
            info.horizontal_resolution,
            info.vertical_resolution,
            info.pixels_per_scan_line,
            info.pixel_format,
        )
    };

    print(con_out, "GOP framebuffer @ ");
    print_u64_hex(con_out, fb_base);
    print(con_out, " size=");
    print_u64_hex(con_out, fb_size as u64);
    print(con_out, "\r\n  resolution=");
    print_u32(con_out, w);
    print(con_out, "x");
    print_u32(con_out, h);
    print(con_out, " stride=");
    print_u32(con_out, stride);
    print(con_out, " pixfmt=");
    print_u32(con_out, pf);
    print(con_out, "\r\n");

    paint_pattern(mode);

    print(con_out, "Loading embedded kernel ELF...\r\n");
    let entry = match load_kernel_elf(bs) {
        Some(entry) => entry,
        None => {
            print(con_out, "Kernel ELF load failed; halting.\r\n");
            halt();
        }
    };
    print(con_out, "Kernel loaded; exiting boot services.\r\n");

    let handoff = exit_boot_services(image, bs, con_out, mode);
    jump_to_kernel(entry, &handoff);
}
