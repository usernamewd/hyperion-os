//! Virtio-blk driver.
//!
//! Implements just enough of the virtio-blk protocol (virtio 1.1 §5.2) to
//! issue 512-byte read/write requests via a single virtqueue. This is
//! used by [`crate::drivers::block`] to expose the disk to the rest of
//! the kernel; the FS layer mounts a tiny block-FS at `/blk`.
//!
//! Handshake:
//! 1. ACK + DRIVER status bits.
//! 2. Negotiate `VIRTIO_F_VERSION_1` only — anything else is optional.
//! 3. Set up requestq (virtqueue 0).
//! 4. DRIVER_OK.
//!
//! Each request is a 3-descriptor chain: header (read-only by device),
//! data buffer, status byte (write-only by device).

use alloc::sync::Arc;
use alloc::vec;

use super::mmio::{Transport, VIRTIO_F_VERSION_1};
use super::queue::{Virtqueue, VRING_DESC_F_NEXT, VRING_DESC_F_WRITE};

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_S_OK: u8 = 0;

const QUEUE_SIZE: u16 = 16;
const SECTOR_SIZE: usize = 512;

#[repr(C)]
struct ReqHeader {
    ty: u32,
    reserved: u32,
    sector: u64,
}

/// One bring-up = one device. The driver lives behind a `Mutex` since
/// virtio-blk on QEMU virt is single-queued.
pub struct VirtioBlk {
    transport: Transport,
    queue: crate::sync::Mutex<Virtqueue>,
    capacity_sectors: u64,
}

impl VirtioBlk {
    /// Number of 512-byte sectors the device exposes.
    pub fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    /// Read `n` consecutive sectors starting at `sector` into `buf`,
    /// which must be at least `n * 512` bytes long.
    pub fn read_sectors(&self, sector: u64, n: usize, buf: &mut [u8]) -> Result<(), &'static str> {
        if buf.len() < n * SECTOR_SIZE {
            return Err("buffer too small");
        }
        self.do_request(VIRTIO_BLK_T_IN, sector, n, buf)
    }

    /// Write `n` consecutive sectors starting at `sector` from `buf`.
    pub fn write_sectors(&self, sector: u64, n: usize, buf: &[u8]) -> Result<(), &'static str> {
        if buf.len() < n * SECTOR_SIZE {
            return Err("buffer too small");
        }
        // We need a mutable buffer for the descriptor pointer (the
        // device only reads it but our `do_request` uses a single byte
        // slice signature). Copy in.
        let mut tmp = vec![0u8; n * SECTOR_SIZE];
        tmp.copy_from_slice(&buf[..n * SECTOR_SIZE]);
        self.do_request(VIRTIO_BLK_T_OUT, sector, n, &mut tmp)
    }

    fn do_request(
        &self,
        ty: u32,
        sector: u64,
        n: usize,
        data: &mut [u8],
    ) -> Result<(), &'static str> {
        let header = ReqHeader {
            ty,
            reserved: 0,
            sector,
        };
        let header_box = alloc::boxed::Box::new(header);
        let mut status: alloc::boxed::Box<u8> = alloc::boxed::Box::new(0xff);

        let mut q = self.queue.lock();
        let head = q.alloc_chain(3).ok_or("virtio-blk: virtqueue full")?;

        // Walk chain to set up each descriptor.
        let mut idx = head;
        // 0: header — device-readable
        q.desc[idx as usize].addr = (&*header_box as *const ReqHeader) as u64;
        q.desc[idx as usize].len = core::mem::size_of::<ReqHeader>() as u32;
        q.desc[idx as usize].flags = VRING_DESC_F_NEXT;
        idx = q.desc[idx as usize].next;

        // 1: data buffer — direction depends on request type.
        let data_flags = if ty == VIRTIO_BLK_T_IN {
            VRING_DESC_F_NEXT | VRING_DESC_F_WRITE
        } else {
            VRING_DESC_F_NEXT
        };
        q.desc[idx as usize].addr = data.as_mut_ptr() as u64;
        q.desc[idx as usize].len = (n * SECTOR_SIZE) as u32;
        q.desc[idx as usize].flags = data_flags;
        idx = q.desc[idx as usize].next;

        // 2: status — device-writable
        q.desc[idx as usize].addr = (&*status as *const u8) as u64;
        q.desc[idx as usize].len = 1;
        q.desc[idx as usize].flags = VRING_DESC_F_WRITE;

        q.push_avail(head);
        drop(q);

        self.transport.queue_notify(0);

        // Wait for the device to complete this request. virtio-blk on
        // QEMU services the queue immediately so this is microseconds.
        let mut q = self.queue.lock();
        let (_id, _len) = q.wait_used();
        q.free_chain(head);
        drop(q);

        let s = *status;
        // Ensure `header_box` lives until the device is done.
        drop(header_box);
        let _ = &mut *status;

        if s == VIRTIO_BLK_S_OK {
            Ok(())
        } else {
            Err("virtio-blk: device error")
        }
    }
}

/// Bring up a virtio-blk device at the given mmio base.
pub fn init(base: usize) -> Result<(), &'static str> {
    // SAFETY: caller has already probed the slot.
    let transport = unsafe { Transport::new(base) };
    transport.init_begin().map_err(|_| "init_begin failed")?;

    let device_feats = transport.read_features();
    // Negotiate just VERSION_1; the device picks up everything else by
    // default.
    let driver_feats = device_feats & VIRTIO_F_VERSION_1;
    transport.write_features(driver_feats);

    let max = transport.queue_max(0);
    if max == 0 {
        return Err("virtio-blk: no requestq");
    }
    let size = QUEUE_SIZE.min(max as u16);
    let mut q = Virtqueue::new(size);
    transport.queue_configure(0, size as u32, q.desc_phys(), q.avail_phys(), q.used_phys());
    // Pre-init the avail/used last_used cursor.
    q.last_used_idx = 0;

    // Read capacity (in 512-byte sectors).
    let capacity = transport.config_read_u64(0);

    transport.driver_ok();

    let dev = Arc::new(VirtioBlk {
        transport,
        queue: crate::sync::Mutex::new(q),
        capacity_sectors: capacity,
    });

    crate::log::info!(
        "virtio-blk: capacity={} sectors ({} MiB)",
        capacity,
        capacity * SECTOR_SIZE as u64 / (1024 * 1024)
    );

    crate::drivers::block::register(dev);
    Ok(())
}
