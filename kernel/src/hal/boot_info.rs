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

/// Hardware-defined identifier for the boot interrupt controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntcKind {
    /// ARM GICv2 (memory-mapped distributor + CPU interface). QEMU virt
    /// default, Cortex-A15/A53 era boards.
    GicV2,
    /// ARM GICv3 (memory-mapped distributor + system-register CPU
    /// interface). Modern server-class ARM and Apple-class CPUs.
    GicV3,
    /// Intel/AMD Local APIC + I/O APIC. The amd64 default. `primary`
    /// is the LAPIC MMIO base; `secondary` is the I/O APIC MMIO base.
    Apic,
    /// Legacy Intel 8259 master/slave PIC pair. Used as a fallback on
    /// pre-APIC x86 hardware. We never use it directly; on amd64 we
    /// always switch to APIC and only mask the PIC off.
    Pic8259,
}

/// Backwards-compatible alias so older code that still references
/// "GIC version" reads cleanly.
pub use IntcKind as GicVersion;

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

pub const MAX_MEMORY_REGIONS: usize = 8;
pub const UEFI_HANDOFF_MAGIC: u64 = 0x4859_5055_4546_4948;
pub const UEFI_HANDOFF_VERSION: u32 = 1;

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

    /// Boot interrupt controller: kind + register windows.
    pub intc: IntcSpec,

    /// Boot timer frequency in Hz. On aarch64 this matches `CNTFRQ_EL0`;
    /// on x86_64 it is the calibrated TSC frequency. 0 means "fall back
    /// to a calibration loop or arch-default."
    pub timer_freq_hz: u32,

    /// Pre-existing framebuffer from firmware, if any.
    pub framebuffer: Option<FramebufferInfo>,

    /// Physical address of the firmware-supplied configuration table:
    /// the device-tree blob on aarch64, the Multiboot2 info struct on
    /// amd64-via-GRUB, or 0 when there isn't one (e.g. plain `-kernel`).
    pub fw_table_addr: u64,
}

/// Backwards-compatible alias kept so callers that grew up with the
/// aarch64-only schema still compile while we migrate them.
impl BootInfo {
    /// DTB address (aarch64) — alias of [`Self::fw_table_addr`].
    #[inline]
    pub fn dtb_addr(&self) -> u64 {
        self.fw_table_addr
    }
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
pub struct IntcSpec {
    pub kind: IntcKind,
    /// GIC distributor on aarch64; LAPIC base on x86_64.
    pub primary: RegSpec,
    /// GICv2 CPU interface or GICv3 redistributor on aarch64; I/O APIC
    /// base on x86_64.
    pub secondary: RegSpec,
}

/// Backwards-compatible alias for the pre-refactor name. Kept so the
/// aarch64 GIC driver doesn't need a sweeping rename.
pub use IntcSpec as GicSpec;

impl IntcSpec {
    /// `intc.dist` was the aarch64 GIC distributor; new code uses
    /// [`Self::primary`].
    #[inline]
    pub fn dist(&self) -> RegSpec {
        self.primary
    }

    /// `intc.cpu_or_redist` was the aarch64 GIC CPU interface /
    /// redistributor; new code uses [`Self::secondary`].
    #[inline]
    pub fn cpu_or_redist(&self) -> RegSpec {
        self.secondary
    }
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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HandoffMemoryRegion {
    pub base: u64,
    pub size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UefiHandoff {
    pub magic: u64,
    pub version: u32,
    pub _reserved0: u32,
    pub console_kind: u32,
    pub console_base: u64,
    pub console_size: u64,
    pub console_clock_hz: u32,
    pub intc_kind: u32,
    pub intc_primary_base: u64,
    pub intc_primary_size: u64,
    pub intc_secondary_base: u64,
    pub intc_secondary_size: u64,
    pub timer_freq_hz: u32,
    pub memory_len: u32,
    pub memory: [HandoffMemoryRegion; MAX_MEMORY_REGIONS],
    pub framebuffer_present: u32,
    pub framebuffer_base: u64,
    pub framebuffer_size: u64,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub framebuffer_stride_bytes: u32,
    pub framebuffer_bpp: u32,
    pub framebuffer_format: u32,
    pub fw_table_addr: u64,
}

impl UefiHandoff {
    pub fn to_boot_info(self) -> Option<BootInfo> {
        if self.magic != UEFI_HANDOFF_MAGIC || self.version != UEFI_HANDOFF_VERSION {
            return None;
        }
        let console_kind = match self.console_kind {
            0 => ConsoleKind::Pl011,
            1 => ConsoleKind::Ns16550,
            2 => ConsoleKind::BcmMiniUart,
            _ => return None,
        };
        let intc_kind = match self.intc_kind {
            0 => IntcKind::GicV2,
            1 => IntcKind::GicV3,
            2 => IntcKind::Apic,
            3 => IntcKind::Pic8259,
            _ => return None,
        };
        let mut memory = MemoryMap::empty();
        let len = (self.memory_len as usize).min(MAX_MEMORY_REGIONS);
        for region in self.memory.iter().take(len) {
            memory.push(MemoryRegion::new(region.base, region.size));
        }
        let framebuffer = if self.framebuffer_present != 0 {
            let format = match self.framebuffer_format {
                0 => PixelFormat::Bgra8888,
                1 => PixelFormat::Rgba8888,
                _ => return None,
            };
            Some(FramebufferInfo {
                base: self.framebuffer_base,
                width: self.framebuffer_width,
                height: self.framebuffer_height,
                stride_bytes: self.framebuffer_stride_bytes,
                bpp: self.framebuffer_bpp,
                format,
            })
        } else {
            None
        };
        Some(BootInfo {
            console: ConsoleSpec {
                kind: console_kind,
                regs: RegSpec::new(self.console_base, self.console_size),
                clock_hz: self.console_clock_hz,
            },
            memory,
            intc: IntcSpec {
                kind: intc_kind,
                primary: RegSpec::new(self.intc_primary_base, self.intc_primary_size),
                secondary: RegSpec::new(self.intc_secondary_base, self.intc_secondary_size),
            },
            timer_freq_hz: self.timer_freq_hz,
            framebuffer,
            fw_table_addr: self.fw_table_addr,
        })
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
            intc: IntcSpec {
                kind: IntcKind::GicV2,
                primary: RegSpec::new(0x0800_0000, 0x10000),
                secondary: RegSpec::new(0x0801_0000, 0x10000),
            },
            timer_freq_hz: 0, // read from CNTFRQ_EL0
            framebuffer: None,
            fw_table_addr: 0,
        }
    }

    /// Compile-time fallback for QEMU's `q35` x86_64 machine — used
    /// only when boot discovery (Multiboot2 / UEFI handoff) couldn't
    /// describe the board. NS16550 COM1 at 0x3F8, LAPIC at the standard
    /// reset base (0xFEE0_0000), I/O APIC at 0xFEC0_0000.
    pub const fn qemu_q35_fallback() -> Self {
        Self {
            console: ConsoleSpec {
                kind: ConsoleKind::Ns16550,
                regs: RegSpec::new(0x3F8, 8),
                clock_hz: 1_843_200,
            },
            memory: {
                let mut m = MemoryMap::empty();
                // 128 MiB starting at the 1 MiB mark. SeaBIOS leaves us
                // 0x1_0000..0xA_0000 of low RAM; the high RAM extends
                // up to whatever the firmware reported. We pick a
                // conservative window so we boot even if Multiboot2
                // tags are missing.
                m.regions[0] = MemoryRegion::new(0x10_0000, 128 * 1024 * 1024);
                m.len = 1;
                m
            },
            intc: IntcSpec {
                kind: IntcKind::Apic,
                primary: RegSpec::new(0xFEE0_0000, 0x1000),
                secondary: RegSpec::new(0xFEC0_0000, 0x1000),
            },
            timer_freq_hz: 0, // calibrate via PIT
            framebuffer: None,
            fw_table_addr: 0,
        }
    }
}

impl fmt::Debug for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfo")
            .field("console", &(self.console.kind, self.console.regs))
            .field("memory", &self.memory)
            .field(
                "intc",
                &(self.intc.kind, self.intc.primary, self.intc.secondary),
            )
            .field("timer_freq_hz", &self.timer_freq_hz)
            .field("framebuffer", &self.framebuffer)
            .field("fw_table_addr", &format_args!("{:#x}", self.fw_table_addr))
            .finish()
    }
}
