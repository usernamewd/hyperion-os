//! SMP (multi-CPU) bring-up via PSCI CPU_ON.
//!
//! On aarch64 the kernel uses the **PSCI** firmware interface (SMC/HVC
//! to the EL2/EL3 monitor) to wake secondary CPUs. QEMU's `virt`
//! machine and most modern ARM firmware (TF-A, U-Boot) implement
//! PSCI 0.2+, so `CPU_ON` is essentially universal.
//!
//! Bring-up flow:
//!
//! 1. Boot CPU finishes early init (HAL, MMU, GIC distributor, timer).
//! 2. Boot CPU calls [`start_secondaries`], which iterates MPIDR aff
//!    values 1..=`max_cpus-1` and issues PSCI CPU_ON pointing each
//!    woken core at [`_start_secondary`] (in `boot.rs`) with `x0`
//!    carrying the secondary's logical CPU id.
//! 3. The secondary lands in `_start_secondary`, drops to EL1, sets
//!    its own per-CPU stack, installs the early exception vector, and
//!    tail-calls [`hyperion_kernel_secondary_main`] below.
//! 4. The Rust trampoline registers the per-CPU id in TPIDR_EL1,
//!    initialises the local GIC interface, programs the local timer,
//!    enables IRQs, marks itself online, and enters [`secondary_idle`].
//!
//! Today the secondaries don't yet pull from the runqueue (the
//! scheduler only owns one `current` thread). They run as cooperative
//! "online idle" CPUs — proof the wakeup path is real, with hooks to
//! plug into a per-CPU scheduler later.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::log;
use crate::proc::percpu::{self, MAX_CPUS};

extern "C" {
    /// Secondary CPU entry point. Defined in `boot.rs`. The aarch64
    /// boot stub places it in `.text.boot` and PSCI lands here in EL1
    /// (or EL2, depending on firmware) with `x0` holding `context_id`.
    fn _start_secondary();
}

static SMP_STARTED: AtomicBool = AtomicBool::new(false);

/// Wake every secondary CPU we know about. Idempotent — calling twice
/// just re-issues PSCI CPU_ON for cores already running, which PSCI
/// reports as `ALREADY_ON` (-4) and we silently ignore.
pub fn start_secondaries() {
    if SMP_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    // Boot CPU registers itself first so logical id 0 is taken.
    percpu::register_boot_cpu(0);
    percpu::mark_online(0);

    // PSCI CPU_ON expects a *physical* entry address. Our identity
    // mapping (caches/MMU off, low RAM at the same VA as PA) means the
    // function symbol address is also the physical address.
    let entry = _start_secondary as usize as u64;

    let mut woken = 0u32;
    for cpu_id in 1..MAX_CPUS as u32 {
        // QEMU's virt machine uses MPIDR aff0 == cpu_id for cores 0..7.
        let target_mpidr = cpu_id as u64;
        let context_id = cpu_id as u64;

        let ret = super::psci::cpu_on(target_mpidr, entry, context_id);
        match ret {
            super::psci::PSCI_SUCCESS => {
                log::info!(
                    "smp: PSCI CPU_ON cpu#{} -> entry={:#x} ok",
                    cpu_id,
                    entry
                );
                woken += 1;
            }
            super::psci::PSCI_INVALID_PARAMETERS | super::psci::PSCI_DENIED => {
                // Probably no such CPU on this machine. Stop probing.
                break;
            }
            super::psci::PSCI_ALREADY_ON => {
                log::info!("smp: cpu#{} already on", cpu_id);
            }
            other => {
                log::warn!("smp: PSCI CPU_ON cpu#{} returned {}", cpu_id, other);
            }
        }
    }

    // Wait briefly for secondaries to come online so the boot log
    // reflects reality. We don't insist on every one — if firmware
    // failed silently we still want the boot CPU to keep going.
    let want = woken + 1;
    for _ in 0..1_000_000 {
        if percpu::online_count() >= want {
            break;
        }
        core::hint::spin_loop();
    }

    log::info!(
        "smp: {}/{} CPUs online (mask={:#x})",
        percpu::online_count(),
        want,
        percpu::online_mask()
    );
}

/// Idle loop run by every secondary CPU once it is online. Pulls work
/// off the shared runqueue when available, otherwise WFIs until the
/// timer ticks or another CPU sends a wake-up SGI.
fn secondary_idle() -> ! {
    loop {
        // Today: cooperative idle. The scheduler only owns one
        // `current` thread on the boot CPU; secondaries can't yet
        // pull threads safely without per-CPU `current` slots. The
        // hook is here for when we add work-stealing.
        crate::arch::idle();
    }
}

/// Rust-side trampoline for secondary CPUs. Called from
/// `_start_secondary` after a per-CPU stack and the early exception
/// vector are set up. `cpu_id` is the logical id passed as
/// `context_id` to PSCI CPU_ON.
#[no_mangle]
pub extern "C" fn hyperion_kernel_secondary_main(cpu_id: u32) -> ! {
    percpu::register_boot_cpu(cpu_id);

    // Install the real exception vector and bring up this CPU's GIC
    // interface + local timer. The distributor was configured by the
    // boot CPU; here we just light up the per-CPU portion.
    super::exceptions::install_vectors();
    super::gic::init_per_cpu();
    super::timer::init();

    super::exceptions::enable_irqs();
    percpu::mark_online(cpu_id);

    log::info!("smp: cpu#{} online (EL{})", cpu_id, super::current_el());

    secondary_idle()
}
