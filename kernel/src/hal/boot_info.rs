//! Hardware-discovery results handed from the boot stub to [`crate::kmain`].
//!
//! The boot stub is responsible for filling this in from whatever boot
//! protocol it speaks (DTB-on-x0 for QEMU virt / U-Boot; UEFI handoff for
//! UEFI hardware; future Pi mailbox; …). The kernel core treats the result
//! as the source of truth and never references board-specific constants.
//!
//! All addresses are physical. The kernel runs identity-mapped, so they're
//! also currently the virtual addresses, but the field naming is
//! deliberately physical so the day we move the kernel into a higher half
//! we don't have to chase down stale assumptions.

use core::fmt;

/// Hardware-defined identifier for a console UART.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleKind {
    /// ARM PL011 PrimeCell (`arm,pl011`, `arm,primecell`). QEMU virt,
    /// many embedded boards, AArch64 servers' BMC consoles.
    Pl011,
    /// 8250-compatible (`ns16550a`, `ns16550`, `snps,dw-apb-uart`,
    /// many SoCs: NXP Layerscape, Allwinner, Rockchip, etc.).
    Ns16550,
    /// Broadcom 2835 mini-UART. Raspberry Pi 3+ default boot console
    /// when `enable_uart=1` is set in `config.txt`.
    BcmMiniUart,
}

/// Hardware-defined identifier for the interrupt controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GicVersion {
    /// GICv2 (memory-mapped distributor + CPU interface). QEMU virt
    /// default, Cortex-A15/A53 era boards.
    V2,
    /// GICv3 (memory-mapped distributor + system-register CPU interface).
    /// Modern server-class ARM and Apple-class CPUs.
    V3,
}

/// A single MMIO range, in physical address space.
#[derive(Debug, Clone, Copy)]
pub struct RegSpec {
    pub base: u64,
    pub size: u64,
}

impl RegSpec {
    pub const fn new(base: u64, size: u64) -> Self {
        Self { base, size }
    }
}

/// A contiguous RAM bank discovered from the boot protocol.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub base: u64,
    pub size: u64,
}

impl MemoryRegion {
    pub const fn new(base: u64, size: u64) -> Self {
        Self { base, size }
    }
}

/// A pre-existing framebuffer the firmware handed us (UEFI GOP, simple-
/// framebuffer DT node, BCM mailbox-allocated framebuffer, …). When
/// present, the display subsystem will register it as monitor #0.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub base: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    /// Bits per pixel; we currently only handle 32 (BGRA8 / RGBA8).
    pub bpp: u32,
    pub format: PixelFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Little-endian: byte 0 = B, 1 = G, 2 = R, 3 = X/A. Matches our
    /// internal compositor format and UEFI `PixelBlueGreenRedReserved8BitPerColor`.
    Bgra8888,
    /// Little-endian: byte 0 = R, 1 = G, 2 = B, 3 = X/A. UEFI
    /// `PixelRedGreenBlueReserved8BitPerColor`.
    Rgba8888,
}

const MAX_MEMORY_REGIONS: usize = 8;

/// Aggregated machine description, passed to [`crate::kmain`] by the boot
/// stub. Construction is fallible (we may discover unsupported hardware),
/// but if you have a [`BootInfo`] in hand it's been sanitised.
#[derive(Clone, Copy)]
pub struct BootInfo {
    /// Console UART base + kind. The boot stub guarantees this is alive
    /// before [`crate::kmain`] runs so logs always have somewhere to go.
    pub console: ConsoleSpec,

    /// RAM banks usable by the kernel. The kernel image, BSS, stack, heap
    /// area and any firmware-reserved regions have already been carved
    /// out by the boot stub before this is published.
    pub memory: MemoryMap,

    /// Generic interrupt controller: version + register windows.
    pub gic: GicSpec,

    /// ARM Generic Timer frequency in Hz (matches `CNTFRQ_EL0`). 0 means
    /// "fall back to reading the register".
    pub timer_freq_hz: u32,

    /// Pre-existing framebuffer from firmware, if any.
    pub framebuffer: Option<FramebufferInfo>,

    /// Physical address of the device tree blob (DTB), if there is one.
    /// Zero means "no DTB" (e.g. UEFI with ACPI-only).
    pub dtb_addr: u64,
}

#[derive(Clone, Copy)]
pub struct ConsoleSpec {
    pub kind: ConsoleKind,
    pub regs: RegSpec,
    /// Optional input clock for divisor calculation. 0 means "use the
    /// driver's default".
    pub clock_hz: u32,
}

#[derive(Clone, Copy)]
pub struct GicSpec {
    pub version: GicVersion,
    pub dist: RegSpec,
    /// GICv2: CPU interface region. GICv3: redistributor region.
    pub cpu_or_redist: RegSpec,
}

#[derive(Clone, Copy)]
pub struct MemoryMap {
    regions: [MemoryRegion; MAX_MEMORY_REGIONS],
    len: usize,
}

impl MemoryMap {
    pub const fn empty() -> Self {
        Self {
            regions: [MemoryRegion::new(0, 0); MAX_MEMORY_REGIONS],
            len: 0,
        }
    }

    /// Append a region. Silently ignores attempts past the cap; the cap
    /// (8) is much larger than any real ARM64 board's bank count.
    pub fn push(&mut self, region: MemoryRegion) {
        if self.len < MAX_MEMORY_REGIONS && region.size > 0 {
            self.regions[self.len] = region;
            self.len += 1;
        }
    }

    pub fn as_slice(&self) -> &[MemoryRegion] {
        &self.regions[..self.len]
    }

    /// Pick the largest single bank as the kernel's primary heap-pool
    /// region. The PMM currently manages a single contiguous range; once
    /// it grows multi-region awareness this can yield all of them.
    pub fn largest(&self) -> Option<MemoryRegion> {
        self.as_slice().iter().copied().max_by_key(|r| r.size)
    }

    /// Total bytes across all regions.
    pub fn total_bytes(&self) -> u64 {
        self.as_slice().iter().map(|r| r.size).sum()
    }
}

impl fmt::Debug for MemoryMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl BootInfo {
    /// Compile-time fallback used when the boot stub couldn't parse a DTB.
    /// Hard-codes the QEMU `virt` machine (the same numbers the kernel
    /// used before the HAL existed). This keeps `qemu-system-aarch64
    /// -kernel hyperion-kernel` working even when no DTB is supplied.
    pub const fn qemu_virt_fallback() -> Self {
        Self {
            console: ConsoleSpec {
                kind: ConsoleKind::Pl011,
                regs: RegSpec::new(0x0900_0000, 0x1000),
                clock_hz: 24_000_000,
            },
            memory: {
                let mut m = MemoryMap::empty();
                // 256 MiB starting at the conventional ARM RAM base. The
                // kernel image lives in the first ~0.5 MiB; the PMM
                // skips past `__kernel_end` automatically.
                m.regions[0] = MemoryRegion::new(0x4000_0000, 256 * 1024 * 1024);
                m.len = 1;
                m
            },
            gic: GicSpec {
                version: GicVersion::V2,
                dist: RegSpec::new(0x0800_0000, 0x10000),
                cpu_or_redist: RegSpec::new(0x0801_0000, 0x10000),
            },
            timer_freq_hz: 0, // read from CNTFRQ_EL0
            framebuffer: None,
            dtb_addr: 0,
        }
    }
}

impl fmt::Debug for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfo")
            .field("console", &(self.console.kind, self.console.regs))
            .field("memory", &self.memory)
            .field("gic", &(self.gic.version, self.gic.dist, self.gic.cpu_or_redist))
            .field("timer_freq_hz", &self.timer_freq_hz)
            .field("framebuffer", &self.framebuffer)
            .field("dtb_addr", &format_args!("{:#x}", self.dtb_addr))
            .finish()
    }
}
