//! Hyperion OS microkernel library.
//!
//! Hyperion is a small microkernel inspired by HarmonyOS / L4-family
//! systems. It runs on aarch64 and amd64/x86_64. It is designed to be
//! **integratable**: the kernel proper provides a minimal trusted
//! computing base (boot, MMU, scheduler, IPC, capabilities) and exposes
//! well-defined extension points for layering on:
//!
//! * Drivers (UART, framebuffer, GPU, etc.)
//! * Filesystems (ramfs in-tree; VFS lets you plug in more)
//! * Display servers (multiple physical & virtual displays via the
//!   compositor)
//! * UI toolkits (built on top of the [`ui`] primitives)
//! * Custom OS distributions (re-export the API surface from
//!   `hyperion-os-api`)
//!
//! The kernel boots on QEMU `virt` aarch64 out of the box and is structured
//! so that adding a new board (Raspberry Pi 4/5, Apple silicon family, etc.)
//! mostly amounts to providing a new `arch::<board>` module and wiring its
//! UART / timer / interrupt controller into the same traits.
//!
//! ## Layout
//!
//! | Module        | Purpose                                              |
//! |---------------|------------------------------------------------------|
//! | [`arch`]      | Architecture-specific code (boot, MMU, exceptions).  |
//! | [`mm`]        | Physical & virtual memory management, kernel heap.   |
//! | [`proc`]      | Threads, processes, address spaces, scheduler.       |
//! | [`ipc`]       | Capability-addressed message ports.                  |
//! | [`fs`]        | Virtual filesystem and the in-memory ramfs.          |
//! | [`display`]   | Framebuffer / monitor / virtual display + compositor.|
//! | [`ui`]        | Canvas + widget primitives for building UIs.         |
//! | [`syscall`]   | Syscall dispatch and number table.                   |
//! | [`shell`]     | Built-in interactive shell, runs as a kernel task.   |
//!
//! ## Stability
//!
//! Everything re-exported from [`hyperion_os_api`] is considered the stable
//! public API for **building OSes on top of Hyperion**. Internal kernel
//! types are not stability-promised.

#![no_std]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::new_without_default)]
#![allow(clippy::needless_return)]
#![allow(clippy::upper_case_acronyms)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod arch;
pub mod display;
pub mod drivers;
pub mod fs;
pub mod hal;
pub mod ipc;
pub mod log;
pub mod mm;
pub mod panic;
pub mod proc;
pub mod shell;
pub mod sync;
pub mod syscall;
pub mod ui;

pub use hyperion_os_api as api;

#[cfg(target_arch = "aarch64")]
const ARCH_NAME: &str = "aarch64";
#[cfg(target_arch = "x86_64")]
const ARCH_NAME: &str = "x86_64";

/// Kernel banner printed on boot.
pub const BANNER: &str = concat!(
    "\r\n",
    "============================================================\r\n",
    "  Hyperion OS  microkernel  v",
    env!("CARGO_PKG_VERSION"),
    "\r\n",
    "  (c) Hyperion contributors. MIT OR Apache-2.0.\r\n",
    "============================================================\r\n",
);

/// Architecture-independent kernel entry point.
///
/// Called from the architecture-specific boot path once a stack is set
/// up, BSS is zeroed, and the HAL has been brought up (which means the
/// boot console is already alive — the boot stub parsed the device-tree
/// blob and instantiated the right UART driver before jumping here).
///
/// Initialisation order matters; see inline comments.
///
/// This function does not return.
pub fn kmain() -> ! {
    // 1. Early console is up via `hal::init` from the boot stub. Wire
    //    the `log` facade so the rest of the kernel can use it.
    crate::log::init();
    log::info!("{}", BANNER);
    log::info!("kmain: entering kernel main");

    // Echo the discovered hardware so the user can see what board the
    // kernel believes it's running on. Useful when porting.
    let bi = hal::info();
    log::info!(
        "hal: arch={} console={:?}@{:#x} intc={:?}@{:#x} fw_table={:#x} ram_total={} MiB",
        ARCH_NAME,
        bi.console.kind,
        bi.console.regs.base,
        bi.intc.kind,
        bi.intc.primary.base,
        bi.fw_table_addr,
        bi.memory.total_bytes() / (1024 * 1024)
    );

    // 2. Memory: physical frame allocator first (so heap can request pages
    //    later if it wants to grow), then the kernel heap, then the MMU.
    //    The MMU is intentionally last because the heap allocator we use
    //    is happy to run with identity mapping until paging is enabled.
    mm::init();
    log::info!("memory: PMM ready, heap online");

    // 3. Architecture-specific late init: exception vectors, timer,
    //    interrupt controller. The arch facade dispatches to whichever
    //    backend is being compiled (aarch64::late_init or
    //    x86_64::late_init).
    arch::late_init();
    log::info!("arch: exceptions + timer + interrupt controller initialised");

    // 4. Process / scheduler bookkeeping.
    proc::init();
    log::info!("proc: scheduler initialised");

    // 5. IPC, filesystem, display.
    ipc::init();
    fs::init();
    display::init();
    log::info!("services: ipc / fs / display initialised");

    // 6. Probe and bring up virtio devices on aarch64 (block, net, gpu).
    //    Each device registers itself with the relevant subsystem
    //    (block-device-backed FS, network stack, display compositor).
    drivers::init_late();
    log::info!("drivers: virtio probe done");

    // 7. Wake the secondary CPUs. On aarch64 this issues PSCI CPU_ON;
    //    on x86_64 it is a no-op (we boot only the BSP for now).
    arch::start_secondaries();

    // 8. Spawn the shell as a kernel task and enter the scheduler.
    shell::spawn();
    log::info!("shell spawned; handing control to the scheduler");

    proc::scheduler::run();
}
