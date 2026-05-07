//! Hyperion OS UEFI boot stub.
//!
//! This is a tiny `aarch64-unknown-uefi` PE that runs as an EFI
//! application. Its job is to discover the firmware-provided graphics
//! output (GOP) and memory map, then hand control to the Hyperion
//! kernel proper.
//!
//! Right now this stub:
//!
//! 1. Locates the **Graphics Output Protocol** (GOP) via Boot Services
//!    and reads the active mode's framebuffer base / resolution /
//!    stride / pixel format.
//! 2. Prints a banner + the framebuffer description to UEFI ConOut so
//!    you can confirm in the firmware console that the stub ran.
//! 3. Paints a recognisable test pattern straight into the GOP
//!    framebuffer (Hyperion stripes + a centred white square). This
//!    proves the framebuffer is real and writable from the stub.
//! 4. Halts.
//!
//! A follow-up iteration will load the kernel ELF (currently produced
//! as `aarch64-unknown-none`), exit boot services, and jump to it with
//! a populated `BootInfo`. The kernel side is already prepared to
//! accept that handover (`hal::init` + `kmain` take a `BootInfo`).
//!
//! The stub is deliberately self-contained — no `uefi` crate
//! dependency, just hand-rolled UEFI types — so it stays small,
//! reviewable, and free of surprise transitive deps.

#![no_std]
#![no_main]

use core::ffi::c_void;
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
    allocate_pages: *const c_void,
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
extern "efiapi" fn efi_main(_image: EFI_HANDLE, system_table: *mut EfiSystemTable) -> EFI_STATUS {
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

    print(con_out, "Test pattern painted; halting.\r\n");
    halt();
}
