//! Virtio device drivers.
//!
//! Hyperion uses **virtio-mmio** as the discovery transport on aarch64
//! QEMU `virt`, which exposes 32 generic mmio slots starting at
//! `0x0a00_0000` (each one is 0x200 bytes). Probing each slot looks at
//! the magic number / device id register set and instantiates the
//! right concrete driver (block, net, gpu).
//!
//! On x86_64 the virtio devices are PCI-discovered. Since we don't
//! ship a PCI bus driver yet, the x86_64 build skips probing — the
//! infrastructure is here for when PCI lands.
//!
//! Each driver is responsible for:
//! 1. Acknowledging the device (FEATURES_OK, DRIVER_OK).
//! 2. Negotiating features it understands.
//! 3. Setting up its virtqueue(s) (descriptor table + avail/used
//!    rings) using the [`mmio`] transport helper.
//! 4. Registering itself with the relevant subsystem
//!    ([`crate::display`], [`crate::drivers::block`],
//!    [`crate::drivers::net`]).

pub mod blk;
pub mod gpu;
pub mod mmio;
pub mod net;
pub mod queue;

/// QEMU virt's virtio-mmio window: 32 slots starting at 0x0a000000,
/// each 0x200 bytes. Empty slots return DEVICE_ID == 0.
const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_COUNT: usize = 32;

/// Probe every virtio-mmio slot and bring up the supported devices.
/// Called once from [`crate::drivers::init_late`].
pub fn probe_all() {
    #[cfg(target_arch = "aarch64")]
    {
        for slot in 0..VIRTIO_MMIO_COUNT {
            let base = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
            // SAFETY: every slot is mapped MMIO; we just read a u32 magic
            // number and bail if it's not virtio.
            let header = unsafe { mmio::Transport::probe(base) };
            let header = match header {
                Some(h) => h,
                None => continue,
            };
            crate::log::info!(
                "virtio: slot#{slot} @ {:#x} device_id={} version={}",
                base,
                header.device_id,
                header.version
            );
            match header.device_id {
                mmio::VIRTIO_ID_BLOCK => {
                    if let Err(e) = blk::init(base) {
                        crate::log::warn!("virtio-blk init failed: {e}");
                    }
                }
                mmio::VIRTIO_ID_NET => {
                    if let Err(e) = net::init(base) {
                        crate::log::warn!("virtio-net init failed: {e}");
                    }
                }
                mmio::VIRTIO_ID_GPU => {
                    if let Err(e) = gpu::init(base) {
                        crate::log::warn!("virtio-gpu init failed: {e}");
                    }
                }
                _ => {
                    // Other virtio device classes (console, entropy,
                    // balloon, …) — skip for now.
                }
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        // Virtio-mmio probing is wired only for the aarch64 QEMU virt
        // machine today; x86_64 will go through PCI in a future
        // iteration.
        let _ = (VIRTIO_MMIO_BASE, VIRTIO_MMIO_STRIDE, VIRTIO_MMIO_COUNT);
    }
}
