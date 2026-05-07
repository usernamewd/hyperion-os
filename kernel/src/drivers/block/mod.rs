//! Block-device subsystem.
//!
//! Block devices live behind a small trait so the FS layer can treat
//! virtio-blk, ramdisks, and any future class (SD/MMC, NVMe, …) the
//! same way. Today only virtio-blk implements [`BlockDevice`]; the
//! generic-only ramdisk path is in [`crate::fs::ramfs`].

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::Mutex;

/// 512-byte sector size. Virtio-blk and most consumer block devices
/// use this regardless of the underlying media.
pub const SECTOR_SIZE: usize = 512;

/// Anything that exposes a flat array of 512-byte sectors.
pub trait BlockDevice: Send + Sync {
    fn capacity_sectors(&self) -> u64;
    fn read_sectors(&self, sector: u64, n: usize, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_sectors(&self, sector: u64, n: usize, buf: &[u8]) -> Result<(), &'static str>;
}

impl BlockDevice for crate::drivers::virtio::blk::VirtioBlk {
    fn capacity_sectors(&self) -> u64 {
        crate::drivers::virtio::blk::VirtioBlk::capacity_sectors(self)
    }
    fn read_sectors(&self, sector: u64, n: usize, buf: &mut [u8]) -> Result<(), &'static str> {
        crate::drivers::virtio::blk::VirtioBlk::read_sectors(self, sector, n, buf)
    }
    fn write_sectors(&self, sector: u64, n: usize, buf: &[u8]) -> Result<(), &'static str> {
        crate::drivers::virtio::blk::VirtioBlk::write_sectors(self, sector, n, buf)
    }
}

static DEVICES: Mutex<Vec<Arc<dyn BlockDevice>>> = Mutex::new(Vec::new());

/// Register a new block device. Drivers call this once they finish
/// their virtio init dance.
pub fn register(dev: Arc<dyn BlockDevice>) {
    let mut v = DEVICES.lock();
    v.push(dev);
    crate::log::info!("block: registered device #{}", v.len() - 1);
    // Also seed the FS with a small block-FS view of this device so
    // `cat /blk/info` works in the shell.
    if v.len() == 1 {
        drop(v);
        seed_blkfs();
    }
}

/// Return the first registered block device, if any.
pub fn primary() -> Option<Arc<dyn BlockDevice>> {
    DEVICES.lock().first().cloned()
}

/// Snapshot for the shell `blk` command.
pub fn snapshot() -> Vec<(usize, u64)> {
    DEVICES
        .lock()
        .iter()
        .enumerate()
        .map(|(i, d)| (i, d.capacity_sectors()))
        .collect()
}

/// Build a tiny `/blk` directory in the ramfs that exposes the primary
/// block device to userland-like consumers (the shell). We only
/// support read-back of a single 512-byte sector and a one-line info
/// file — the FS-on-block layer is the right place for richer parsing
/// (ext2 / littlefs) and is intentionally left as future work.
fn seed_blkfs() {
    let dev = match primary() {
        Some(d) => d,
        None => return,
    };
    let cap = dev.capacity_sectors();

    let info = alloc::format!(
        "block device 0\nsectors: {cap}\nsize: {} bytes\n",
        cap * SECTOR_SIZE as u64
    );

    let mut sector0 = [0u8; SECTOR_SIZE];
    let head = match dev.read_sectors(0, 1, &mut sector0) {
        Ok(()) => &sector0[..],
        Err(_) => &[][..],
    };

    let root = &crate::fs::ROOT;
    let _ = root.create_dir("blk");
    if let Ok(blk) = root.resolve("/blk") {
        blk.create_file("info", info.as_bytes()).ok();
        blk.create_file("sector0", head).ok();
    }
}
