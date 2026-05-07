//! Minimal Flattened Device Tree (FDT v17) parser.
//!
//! Just enough to discover what the rest of the kernel needs at boot:
//!
//! * `/chosen` (`bootargs`, `linux,initrd-start/end`, `stdout-path`)
//! * `/memory@*` nodes
//! * `/cpus/cpu@*` (mostly to know we exist)
//! * The interrupt controller (`/interrupt-controller@*` /
//!   `/intc@*`) — `arm,gic-v3` vs. `arm,cortex-a15-gic` /
//!   `arm,gic-400` decides v2 vs. v3.
//! * The console UART selected by `/chosen/stdout-path`, or the first
//!   compatible serial node if `stdout-path` is absent.
//! * Optional `/chosen/framebuffer` (UEFI / `simple-framebuffer`).
//!
//! The parser is allocation-free and `no_std`. It's deliberately permissive
//! about unknown nodes and gracefully degrades to the fallback
//! [`BootInfo::qemu_virt_fallback`] if the blob is missing or malformed —
//! which keeps the existing `-kernel` boot path working.

use super::boot_info::{
    BootInfo, ConsoleKind, ConsoleSpec, FramebufferInfo, GicSpec, GicVersion, MemoryMap,
    MemoryRegion, PixelFormat, RegSpec,
};

const FDT_MAGIC: u32 = 0xd00d_feed;

const FDT_BEGIN_NODE: u32 = 0x0000_0001;
const FDT_END_NODE: u32 = 0x0000_0002;
const FDT_PROP: u32 = 0x0000_0003;
const FDT_NOP: u32 = 0x0000_0004;
const FDT_END: u32 = 0x0000_0009;

#[repr(C)]
struct FdtHeader {
    magic: u32,
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_rsvmap: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

/// Read a 32-bit big-endian value from `bytes` at `offset` (panic-safe;
/// returns `None` on OOB or misalignment).
fn be32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    if end > bytes.len() {
        return None;
    }
    Some(u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn be64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    if end > bytes.len() {
        return None;
    }
    Some(u64::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ]))
}

/// Read a NUL-terminated string from `bytes` starting at `offset`.
fn cstr(bytes: &[u8], offset: usize) -> &str {
    if offset >= bytes.len() {
        return "";
    }
    let tail = &bytes[offset..];
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    core::str::from_utf8(&tail[..end]).unwrap_or("")
}

/// Map a slice over a physical address range. Identity-mapped, so the
/// returned slice points at the same physical bytes the firmware passed.
///
/// # Safety
/// `addr` must be readable for `len` bytes for the lifetime `'a`.
unsafe fn slice_at<'a>(addr: u64, len: usize) -> &'a [u8] {
    // SAFETY: caller pinky-promise.
    unsafe { core::slice::from_raw_parts(addr as *const u8, len) }
}

/// Parse a flattened device tree at `dtb_addr` and return a populated
/// [`BootInfo`]. Falls back to [`BootInfo::qemu_virt_fallback`] for any
/// fields the DTB doesn't tell us about.
///
/// # Safety
/// `dtb_addr` must point to readable memory for the full image (we read
/// `totalsize` from the header first and then validate before reading
/// further). Address `0` is treated as "no DTB" and short-circuits to the
/// fallback.
pub unsafe fn parse_or_fallback(dtb_addr: u64) -> BootInfo {
    let mut info = BootInfo::qemu_virt_fallback();
    info.dtb_addr = dtb_addr;
    if dtb_addr == 0 {
        return info;
    }

    // SAFETY: caller asserts readability; we re-validate by checking the
    // magic before accessing further bytes.
    let header_slice = unsafe { slice_at(dtb_addr, core::mem::size_of::<FdtHeader>()) };
    let magic = match be32(header_slice, 0) {
        Some(v) => v,
        None => return info,
    };
    if magic != FDT_MAGIC {
        return info;
    }
    let totalsize = match be32(header_slice, 4) {
        Some(v) if v >= core::mem::size_of::<FdtHeader>() as u32 => v as usize,
        _ => return info,
    };

    // SAFETY: we now know the firmware advertises a totalsize and a magic.
    let blob = unsafe { slice_at(dtb_addr, totalsize) };

    let off_struct = match be32(blob, 8) {
        Some(v) => v as usize,
        None => return info,
    };
    let off_strings = match be32(blob, 12) {
        Some(v) => v as usize,
        None => return info,
    };
    let size_struct = match be32(blob, 36) {
        Some(v) => v as usize,
        None => return info,
    };
    let strings_size = match be32(blob, 32) {
        Some(v) => v as usize,
        None => return info,
    };
    if off_struct.saturating_add(size_struct) > totalsize
        || off_strings.saturating_add(strings_size) > totalsize
    {
        return info;
    }
    let struct_block = &blob[off_struct..off_struct + size_struct];
    let strings_block = &blob[off_strings..off_strings + strings_size];

    // ---- Walk the tree ----
    walk(struct_block, strings_block, &mut info);

    info
}

/// Path component context kept while walking. Tracks the last few node
/// names so we can match e.g. "/memory@40000000" or "/cpus/cpu@0".
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, Default)]
struct WalkCtx<'a> {
    depth: usize,
    /// `address-cells` / `size-cells` set by the parent node, in CPU
    /// addresses (defaulting to 2/1 per the spec).
    address_cells: [u32; MAX_DEPTH],
    size_cells: [u32; MAX_DEPTH],
    /// The most-recent compatible string seen at the current node.
    cur_compatible: &'a str,
    /// `/chosen/stdout-path` if seen.
    stdout_path: &'a str,
    /// True if we are inside `/chosen`.
    in_chosen: bool,
    /// Current node name (e.g. "memory@40000000" or "pl011@9000000").
    cur_name: &'a str,
}

fn walk(struct_block: &[u8], strings_block: &[u8], info: &mut BootInfo) {
    let mut ctx = WalkCtx::default();
    // Spec defaults at the root.
    ctx.address_cells[0] = 2;
    ctx.size_cells[0] = 1;

    let mut cursor = 0usize;
    // Per-node properties we need to gather then act on at FDT_END_NODE.
    let mut node_compatible: [&str; MAX_DEPTH] = [""; MAX_DEPTH];
    let mut node_reg: [Option<(u64, u64)>; MAX_DEPTH] = [None; MAX_DEPTH];
    let mut node_clock: [u32; MAX_DEPTH] = [0; MAX_DEPTH];
    let mut node_name: [&str; MAX_DEPTH] = [""; MAX_DEPTH];
    let mut fb_props: [Option<FramebufferInfo>; MAX_DEPTH] = [None; MAX_DEPTH];

    // Two-pass-style walking is simpler if we accumulate per-frame state
    // and commit at the closing token.
    while cursor + 4 <= struct_block.len() {
        let token = match be32(struct_block, cursor) {
            Some(t) => t,
            None => return,
        };
        cursor += 4;

        match token {
            FDT_BEGIN_NODE => {
                let name = cstr(struct_block, cursor);
                cursor += name.len() + 1;
                cursor = (cursor + 3) & !3;
                if ctx.depth + 1 >= MAX_DEPTH {
                    // Skip subtree: walk forward counting BEGIN/END.
                    let mut nesting = 1usize;
                    while nesting > 0 && cursor + 4 <= struct_block.len() {
                        let t = match be32(struct_block, cursor) {
                            Some(t) => t,
                            None => return,
                        };
                        cursor += 4;
                        match t {
                            FDT_BEGIN_NODE => {
                                let n = cstr(struct_block, cursor);
                                cursor += n.len() + 1;
                                cursor = (cursor + 3) & !3;
                                nesting += 1;
                            }
                            FDT_END_NODE => nesting -= 1,
                            FDT_PROP => {
                                let plen = match be32(struct_block, cursor) {
                                    Some(v) => v as usize,
                                    None => return,
                                };
                                cursor += 8 + plen;
                                cursor = (cursor + 3) & !3;
                            }
                            FDT_NOP => {}
                            FDT_END => return,
                            _ => return,
                        }
                    }
                    continue;
                }
                ctx.depth += 1;
                // Default address/size cells inherited from parent.
                let parent = ctx.depth - 1;
                ctx.address_cells[ctx.depth] = ctx.address_cells[parent];
                ctx.size_cells[ctx.depth] = ctx.size_cells[parent];
                node_compatible[ctx.depth] = "";
                node_reg[ctx.depth] = None;
                node_clock[ctx.depth] = 0;
                node_name[ctx.depth] = name;
                fb_props[ctx.depth] = None;
                ctx.cur_name = name;
                ctx.in_chosen = ctx.in_chosen || name == "chosen";
            }
            FDT_END_NODE => {
                if ctx.depth == 0 {
                    return;
                }
                act_on_node(
                    info,
                    node_name[ctx.depth],
                    node_compatible[ctx.depth],
                    node_reg[ctx.depth],
                    node_clock[ctx.depth],
                    fb_props[ctx.depth],
                    &ctx,
                );
                if node_name[ctx.depth] == "chosen" {
                    ctx.in_chosen = false;
                }
                ctx.depth -= 1;
            }
            FDT_PROP => {
                let plen = match be32(struct_block, cursor) {
                    Some(v) => v as usize,
                    None => return,
                };
                let pnameoff = match be32(struct_block, cursor + 4) {
                    Some(v) => v as usize,
                    None => return,
                };
                cursor += 8;
                let value_end = cursor + plen;
                if value_end > struct_block.len() {
                    return;
                }
                let value = &struct_block[cursor..value_end];
                let pname = cstr(strings_block, pnameoff);

                handle_prop(
                    pname,
                    value,
                    ctx.depth,
                    &mut ctx,
                    &mut node_compatible,
                    &mut node_reg,
                    &mut node_clock,
                    &mut fb_props,
                );

                cursor = (value_end + 3) & !3;
            }
            FDT_NOP => {}
            FDT_END => return,
            _ => return,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_prop<'a>(
    pname: &'a str,
    value: &'a [u8],
    depth: usize,
    ctx: &mut WalkCtx<'a>,
    node_compatible: &mut [&'a str; MAX_DEPTH],
    node_reg: &mut [Option<(u64, u64)>; MAX_DEPTH],
    node_clock: &mut [u32; MAX_DEPTH],
    fb_props: &mut [Option<FramebufferInfo>; MAX_DEPTH],
) {
    match pname {
        "#address-cells" => {
            if let Some(v) = be32(value, 0) {
                ctx.address_cells[depth] = v;
            }
        }
        "#size-cells" => {
            if let Some(v) = be32(value, 0) {
                ctx.size_cells[depth] = v;
            }
        }
        "compatible" => {
            // First NUL-separated string.
            let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
            let s = core::str::from_utf8(&value[..end]).unwrap_or("");
            node_compatible[depth] = s;
            ctx.cur_compatible = s;
        }
        "reg" => {
            // Use the parent's address/size cells.
            let parent = depth.saturating_sub(1);
            let ac = ctx.address_cells[parent].max(1) as usize;
            let sc = ctx.size_cells[parent].max(1) as usize;
            let entry_words = ac + sc;
            if entry_words == 0 {
                return;
            }
            let need = entry_words * 4;
            if value.len() < need {
                return;
            }
            let addr = read_cells(value, 0, ac);
            let size = read_cells(value, ac * 4, sc);
            node_reg[depth] = Some((addr, size));
        }
        "clock-frequency" => {
            if let Some(v) = be32(value, 0) {
                node_clock[depth] = v;
            }
        }
        "stdout-path" => {
            if depth >= 1 && ctx.in_chosen {
                let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
                if let Ok(s) = core::str::from_utf8(&value[..end]) {
                    ctx.stdout_path = s;
                }
            }
        }
        // simple-framebuffer / chosen,framebuffer properties
        "width" => fb_set(fb_props, depth, |fb| {
            fb.width = be32(value, 0).unwrap_or(0);
        }),
        "height" => fb_set(fb_props, depth, |fb| {
            fb.height = be32(value, 0).unwrap_or(0);
        }),
        "stride" => fb_set(fb_props, depth, |fb| {
            fb.stride_bytes = be32(value, 0).unwrap_or(0);
        }),
        "format" => fb_set(fb_props, depth, |fb| {
            let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
            if let Ok(s) = core::str::from_utf8(&value[..end]) {
                fb.format = match s {
                    "a8b8g8r8" | "x8b8g8r8" | "r8g8b8a8" | "r8g8b8x8" => PixelFormat::Rgba8888,
                    _ => PixelFormat::Bgra8888,
                };
                fb.bpp = 32;
            }
        }),
        _ => {}
    }
}

fn fb_set(
    fb_props: &mut [Option<FramebufferInfo>; MAX_DEPTH],
    depth: usize,
    f: impl FnOnce(&mut FramebufferInfo),
) {
    let entry = fb_props[depth].get_or_insert(FramebufferInfo {
        base: 0,
        width: 0,
        height: 0,
        stride_bytes: 0,
        bpp: 32,
        format: PixelFormat::Bgra8888,
    });
    f(entry);
}

fn read_cells(bytes: &[u8], offset: usize, cells: usize) -> u64 {
    match cells {
        1 => be32(bytes, offset).unwrap_or(0) as u64,
        2 => be64(bytes, offset).unwrap_or(0),
        _ => 0,
    }
}

fn act_on_node(
    info: &mut BootInfo,
    name: &str,
    compatible: &str,
    reg: Option<(u64, u64)>,
    clock_hz: u32,
    fb: Option<FramebufferInfo>,
    ctx: &WalkCtx<'_>,
) {
    // memory@*
    if name.starts_with("memory") {
        if let Some((base, size)) = reg {
            // Replace the fallback's first push the *first* time we see a
            // real memory node, to avoid keeping the QEMU-virt 256 MiB
            // default that lives in the fallback.
            if info.dtb_addr != 0 && info.memory.as_slice().len() == 1 && info.memory.as_slice()[0].base == 0x4000_0000 && info.memory.as_slice()[0].size == 256 * 1024 * 1024 {
                info.memory = MemoryMap::empty();
            }
            info.memory.push(MemoryRegion::new(base, size));
        }
        return;
    }

    // GIC: pick by compatible, irrespective of node name.
    if compatible_matches(compatible, &["arm,gic-v3"]) {
        if let Some((base, size)) = reg {
            info.gic = GicSpec {
                version: GicVersion::V3,
                dist: RegSpec::new(base, size),
                cpu_or_redist: info.gic.cpu_or_redist,
            };
        }
        return;
    }
    if compatible_matches(
        compatible,
        &[
            "arm,gic-400",
            "arm,cortex-a15-gic",
            "arm,cortex-a9-gic",
            "arm,gic",
        ],
    ) {
        if let Some((base, size)) = reg {
            info.gic = GicSpec {
                version: GicVersion::V2,
                dist: RegSpec::new(base, size),
                cpu_or_redist: info.gic.cpu_or_redist,
            };
        }
        return;
    }

    // CPU timer
    if compatible_matches(compatible, &["arm,armv8-timer", "arm,armv7-timer"]) && clock_hz != 0 {
        info.timer_freq_hz = clock_hz;
        return;
    }

    // UART selection. Match the most-specific compatible we know.
    let kind = uart_kind_for(compatible);
    if let Some(kind) = kind {
        if let Some((base, size)) = reg {
            // If `stdout-path` selects a different node, only override if
            // the path's node-name matches.
            if !ctx.stdout_path.is_empty() {
                if path_matches_node(ctx.stdout_path, name) {
                    info.console = ConsoleSpec {
                        kind,
                        regs: RegSpec::new(base, size),
                        clock_hz,
                    };
                }
            } else if info.dtb_addr != 0
                && info.console.regs.base == 0x0900_0000
                && info.console.kind == ConsoleKind::Pl011
            {
                // No stdout-path: take the first compatible UART we see,
                // overriding the fallback.
                info.console = ConsoleSpec {
                    kind,
                    regs: RegSpec::new(base, size),
                    clock_hz,
                };
            }
        }
        return;
    }

    // simple-framebuffer or `/chosen/framebuffer`
    if (compatible == "simple-framebuffer"
        || (ctx.in_chosen && name.starts_with("framebuffer")))
        && fb.is_some()
    {
        if let (Some(mut fb), Some((base, _))) = (fb, reg) {
            fb.base = base;
            if fb.stride_bytes == 0 && fb.bpp != 0 && fb.width != 0 {
                fb.stride_bytes = fb.width.saturating_mul(fb.bpp / 8);
            }
            info.framebuffer = Some(fb);
        }
    }
}

fn uart_kind_for(compatible: &str) -> Option<ConsoleKind> {
    if compatible_matches(compatible, &["arm,pl011", "arm,primecell"]) {
        return Some(ConsoleKind::Pl011);
    }
    if compatible_matches(
        compatible,
        &[
            "ns16550",
            "ns16550a",
            "snps,dw-apb-uart",
            "fsl,16550",
            "nvidia,tegra20-uart",
        ],
    ) {
        return Some(ConsoleKind::Ns16550);
    }
    if compatible_matches(
        compatible,
        &["brcm,bcm2835-aux-uart", "brcm,bcm2837-aux-uart"],
    ) {
        return Some(ConsoleKind::BcmMiniUart);
    }
    None
}

fn compatible_matches(compatible: &str, wanted: &[&str]) -> bool {
    wanted.iter().any(|w| *w == compatible)
}

/// Crude `stdout-path` matcher. Real DT paths may be `/soc/serial@…` with
/// aliases; we just check if the path's last component starts with the
/// node name. Good enough for QEMU virt's `serial0:115200n8` handle and
/// for `/pl011@9000000` style paths.
fn path_matches_node(stdout_path: &str, node_name: &str) -> bool {
    if stdout_path.is_empty() || node_name.is_empty() {
        return false;
    }
    // Strip trailing options like ":115200n8".
    let path = stdout_path.split(':').next().unwrap_or("");
    // Last component.
    let last = path.rsplit('/').next().unwrap_or("");
    if last == node_name {
        return true;
    }
    // Aliases like "serial0" — accept if the node base name matches one of
    // the well-known serial aliases. Without parsing /aliases this is
    // heuristic but suffices for the QEMU virt machine.
    matches!(last, "serial0" | "serial1" | "serial2") && node_name.contains("serial")
        || node_name.contains(last)
        || last.contains(node_name.split('@').next().unwrap_or(""))
}
