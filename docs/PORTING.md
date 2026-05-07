# Porting Hyperion to a new ARM64 board

Hyperion is intentionally board-agnostic at the kernel level. Everything
that varies between platforms — UART address, IRQ controller version,
timer frequency, framebuffer location, RAM layout — is discovered at
runtime through the **HAL** (`kernel/src/hal/`), populated either by:

1. parsing the **device tree blob (DTB)** that firmware passes in `x0`,
   or
2. handover from a UEFI loader that fills `BootInfo` directly (memory
   map from `GetMemoryMap`, framebuffer from GOP, etc.), or
3. the compiled-in **`BootInfo::qemu_virt_fallback()`** so QEMU `-kernel`
   "just works" with no firmware.

This document is the checklist for adding a new board.

## The HAL contract

`kernel/src/hal/boot_info.rs` defines what the kernel needs to know
before `kmain` can do anything useful:

```rust
pub struct BootInfo {
    pub console: ConsoleSpec,        // which UART, where, what stride / clock
    pub gic: GicSpec,                // v2 vs v3, distributor + cpu/redist regs
    pub timer_hz: u32,               // CNTFRQ_EL0 value
    pub memory: MemoryMap,           // usable RAM banks, reserved regions
    pub framebuffer: Option<FramebufferInfo>, // firmware fb (GOP / simple-framebuffer)
    pub dtb: Option<u64>,            // physical address of the DTB if any
}
```

`BootInfo` is parsed once during very early boot, frozen with
`hal::init(bi)`, and exposed read-only via `hal::info()` for the rest
of the kernel's lifetime.

## Wiring path 1: device-tree-described boards

If your firmware (U-Boot, custom bootloader, EDK II, …) hands the
kernel a flattened device tree, **you do not need to write any board
code**. The DTB parser in `kernel/src/hal/dtb.rs` will:

| Property                                                   | Used for                                            |
|------------------------------------------------------------|-----------------------------------------------------|
| `/memory@*/reg`                                            | RAM banks → PMM                                     |
| `/chosen/stdout-path`                                      | which `*serial*` node is the console                |
| `compatible = "arm,pl011"`                                 | PL011 driver                                        |
| `compatible = "ns16550", "ns16550a", "snps,dw-apb-uart"`   | 8250 / 16550 driver                                 |
| `compatible = "brcm,bcm2835-aux-uart"`                     | Pi 3+ mini-UART driver                              |
| `compatible = "arm,gic-400"` / `"arm,cortex-a15-gic"`      | GICv2                                               |
| `compatible = "arm,gic-v3"`                                | GICv3                                               |
| `/timer/clock-frequency` (or `arch_timer`)                 | timer rate                                          |
| `/chosen/framebuffer*` / `simple-framebuffer`              | firmware framebuffer (registered as monitor #0)     |

To add support for a new SoC family that uses any of the above, the
work reduces to:

1. **Console.** If the SoC's UART is a 16550 variant with a different
   register stride, add the `compatible` string to `dtb.rs::pick_uart`
   and pass the right stride to `Ns16550::new(...)`. If it's a brand-new
   IP, add a driver under `kernel/src/drivers/uart/`, add a variant to
   `BootConsole` in `kernel/src/hal/console.rs`, and route from
   `dtb.rs`.
2. **IRQ controller.** GICv2 and GICv3 are already supported. Anything
   else (e.g. PL192 VIC) needs a new driver in
   `kernel/src/arch/aarch64/gic/`.
3. **Timer.** Use the architectural `cntv_*` registers (already done).
   Only override if the SoC uses a memory-mapped timer instead.

Most of the popular ARM64 SoCs (NXP i.MX8/Layerscape, Marvell Armada,
Allwinner H6/A64, Rockchip RK33xx/RK35xx, Ampere eMAG/Altra, Amlogic,
Broadcom BCM27xx with UEFI) already match one of the existing
compatible strings above and need **zero new code** — they just work.

## Wiring path 2: UEFI boards

For boards that ship UEFI firmware (most ARM64 servers, Pi 4/5 with the
Tianocore UEFI build, NXP / Marvell dev boards with EDK II, …) the
boot path goes through `efi-stub/` instead of straight `_start`:

1. The stub runs as a normal `aarch64-unknown-uefi` PE/COFF
   application loaded by the firmware (or written to
   `EFI/BOOT/BOOTAA64.EFI` on the ESP).
2. It uses **Boot Services** to discover hardware: `LocateProtocol`
   for `EFI_GRAPHICS_OUTPUT_PROTOCOL` (framebuffer) and `GetMemoryMap`
   for RAM.
3. It loads the embedded kernel ELF into RAM, builds the UEFI handoff
   block, calls `ExitBootServices`, then jumps to the kernel entry.

The handoff block mirrors the fields the kernel needs in `BootInfo`:
console defaults for QEMU virt, conventional RAM regions, GIC defaults,
and the GOP framebuffer registered as monitor `fb0`.

## Wiring path 3: the QEMU `-kernel` shortcut

`BootInfo::qemu_virt_fallback()` is a compile-time constant with the
QEMU `virt` machine's defaults:

- PL011 @ `0x0900_0000`
- GICv2: dist `0x0800_0000`, cpu IF `0x0801_0000`
- timer 62.5 MHz
- 256 MiB RAM @ `0x4000_0000`

Used when `x0` is null (no DTB) and firmware did not call
`hal::init`. Lets `qemu-system-aarch64 -M virt -kernel hyperion-kernel`
work without any boot infrastructure at all.

## Adding a brand-new board step by step

Worked example: hypothetical SoC "Foo One" with PL011 UART at
`0x0210_0000`, GICv3, 4 GiB RAM at `0x8000_0000`, framebuffer at
`0xF000_0000` (1920x1080 BGRX).

1. **Boot via U-Boot or your bootloader** that produces a DTB with the
   above layout. Hyperion will pick everything up automatically — no
   code changes.
2. Or, if you prefer hardcoded support, add a new constructor next to
   `qemu_virt_fallback`:

   ```rust
   impl BootInfo {
       pub const fn foo_one_fallback() -> Self {
           Self {
               console: ConsoleSpec { kind: ConsoleKind::Pl011,
                   regs: RegSpec { base: 0x0210_0000, size: 0x1000 },
                   stride: 4, baud: 115200, clock_hz: 24_000_000 },
               gic: GicSpec { version: GicVersion::V3,
                   dist: RegSpec { base: 0x0800_0000, size: 0x10000 },
                   cpu_if: RegSpec { base: 0, size: 0 } },
               timer_hz: 24_000_000,
               memory: MemoryMap::single(0x8000_0000, 0x1_0000_0000),
               framebuffer: Some(FramebufferInfo {
                   base: 0xF000_0000, size_bytes: 1920 * 1080 * 4,
                   width: 1920, height: 1080, stride_bytes: 1920 * 4,
                   bpp: 32, format: PixelFormat::Bgra8888,
               }),
               dtb: None,
           }
       }
   }
   ```
3. Select it in `boot.rs` for the `foo-one` cargo feature, or just
   pass the right DTB and skip step 2.

That's it — the rest of the kernel (PMM, scheduler, IPC, ramfs, shell,
display, compositor) is portable and runs unchanged.

## Limits / things you still have to write

- **PSCI quirks.** The shell's `shutdown` / `reboot` use PSCI
  `SYSTEM_OFF` / `SYSTEM_RESET` via SMC. Boards without PSCI need a
  platform-specific power driver.
- **Cache / MMU policy.** Kernel currently runs identity-mapped with
  the MMU off. Going past 1 GiB of address space needs you to enable
  the MMU and program TCR/TTBR for the platform (see `mmu.rs`).
- **SMP.** Secondary CPU wakeup (PSCI `CPU_ON` or spin-table) is on
  the roadmap. Until then only the boot CPU is online.
- **DMA.** No IOMMU/SMMU support yet.
