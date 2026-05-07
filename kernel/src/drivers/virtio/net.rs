//! Virtio-net driver.
//!
//! Implements just enough of virtio-net (virtio 1.1 §5.1) to send and
//! receive Ethernet frames using receiveq + transmitq. There is no
//! checksum offload, no multi-queue, no control queue — features
//! advertised but not negotiated. The packet format prepends a 12-byte
//! `virtio_net_hdr_v1` to each frame on both sides; we leave it
//! zeroed.
//!
//! Driver flow:
//! 1. ACK + DRIVER + negotiate `VIRTIO_F_VERSION_1`.
//! 2. Read MAC from device config.
//! 3. Allocate receiveq (queue 0) and transmitq (queue 1).
//! 4. Pre-fill receiveq with empty buffers so the device has somewhere
//!    to put incoming frames.
//! 5. DRIVER_OK.
//! 6. Receive: scan the receive used-ring on demand (poll-mode for now;
//!    the IRQ path can be added on top).
//!
//! There's a tiny in-kernel UDP echo demo wired through the `net`
//! subsystem (see [`crate::drivers::net::poll_echo`]).

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::mmio::{Transport, VIRTIO_F_VERSION_1};
use super::queue::{Virtqueue, VRING_DESC_F_WRITE};
use crate::sync::Mutex;

const QUEUE_RX: u32 = 0;
const QUEUE_TX: u32 = 1;
const QUEUE_SIZE: u16 = 16;
const MTU: usize = 1514; // 1500 payload + 14 ether header
const RX_BUF_SIZE: usize = MTU + 12; // + virtio_net_hdr

#[repr(C)]
struct VirtioNetHdr {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffers: u16,
}

const HDR_BYTES: usize = core::mem::size_of::<VirtioNetHdr>();

pub struct VirtioNet {
    transport: Transport,
    rx: Mutex<Virtqueue>,
    tx: Mutex<Virtqueue>,
    /// Pre-allocated rx buffers, one per descriptor slot. Indexed by
    /// descriptor head id.
    rx_bufs: Mutex<Vec<Box<[u8]>>>,
    mac: [u8; 6],
}

impl VirtioNet {
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    pub fn send(&self, frame: &[u8]) -> Result<usize, &'static str> {
        if frame.len() > MTU {
            return Err("frame too large");
        }
        // Build a heap buffer = hdr (zeroed) + frame bytes.
        let mut buf: Vec<u8> = vec![0u8; HDR_BYTES + frame.len()];
        buf[HDR_BYTES..].copy_from_slice(frame);
        let len = buf.len() as u32;
        let addr = buf.as_ptr() as u64;

        let mut q = self.tx.lock();
        let head = q.alloc_chain(1).ok_or("virtqueue full")?;
        q.desc[head as usize].addr = addr;
        q.desc[head as usize].len = len;
        q.desc[head as usize].flags = 0;
        q.push_avail(head);
        drop(q);

        self.transport.queue_notify(QUEUE_TX);

        // Wait for completion. Slow but simplest; real drivers reap
        // asynchronously from the IRQ.
        let mut q = self.tx.lock();
        q.wait_used();
        q.free_chain(head);
        drop(q);
        // Hold `buf` alive until here.
        drop(buf);
        Ok(frame.len())
    }

    pub fn recv(&self, out: &mut [u8]) -> Option<usize> {
        let mut q = self.rx.lock();
        let (id, len) = q.pop_used()?;
        let head = id as u16;
        let bufs = self.rx_bufs.lock();
        let b = &bufs[head as usize];
        let n = (len as usize).saturating_sub(HDR_BYTES);
        let n = n.min(out.len());
        out[..n].copy_from_slice(&b[HDR_BYTES..HDR_BYTES + n]);
        drop(bufs);
        // Re-fill this slot.
        let bufs = self.rx_bufs.lock();
        let addr = bufs[head as usize].as_ptr() as u64;
        drop(bufs);
        q.desc[head as usize].addr = addr;
        q.desc[head as usize].len = RX_BUF_SIZE as u32;
        q.desc[head as usize].flags = VRING_DESC_F_WRITE;
        q.push_avail(head);
        drop(q);
        self.transport.queue_notify(QUEUE_RX);
        Some(n)
    }
}

pub fn init(base: usize) -> Result<(), &'static str> {
    // SAFETY: caller probed the slot.
    let transport = unsafe { Transport::new(base) };
    transport.init_begin().map_err(|_| "init_begin failed")?;

    let device_feats = transport.read_features();
    transport.write_features(device_feats & VIRTIO_F_VERSION_1);

    // MAC lives in config bytes 0..6 when VIRTIO_NET_F_MAC is set, which
    // QEMU always advertises.
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = transport.config_read_u8(i);
    }

    let max = transport.queue_max(QUEUE_RX);
    if max == 0 {
        return Err("virtio-net: no rx queue");
    }
    let size = QUEUE_SIZE.min(max as u16);

    // RX queue: pre-fill every descriptor with a buffer.
    let mut rx = Virtqueue::new(size);
    let mut rx_bufs: Vec<Box<[u8]>> = Vec::with_capacity(size as usize);
    for i in 0..size {
        let buf: Box<[u8]> = vec![0u8; RX_BUF_SIZE].into_boxed_slice();
        rx.desc[i as usize].addr = buf.as_ptr() as u64;
        rx.desc[i as usize].len = RX_BUF_SIZE as u32;
        rx.desc[i as usize].flags = VRING_DESC_F_WRITE;
        rx.desc[i as usize].next = 0;
        rx_bufs.push(buf);
    }
    rx.num_free = 0;
    rx.free_head = 0;
    transport.queue_configure(
        QUEUE_RX,
        size as u32,
        rx.desc_phys(),
        rx.avail_phys(),
        rx.used_phys(),
    );
    // Hand every descriptor to the device.
    for i in 0..size {
        rx.push_avail(i);
    }

    // TX queue: empty free list, allocator hands slots out as needed.
    let tx = Virtqueue::new(size);
    transport.queue_configure(
        QUEUE_TX,
        size as u32,
        tx.desc_phys(),
        tx.avail_phys(),
        tx.used_phys(),
    );

    transport.driver_ok();

    let dev = Arc::new(VirtioNet {
        transport,
        rx: Mutex::new(rx),
        tx: Mutex::new(tx),
        rx_bufs: Mutex::new(rx_bufs),
        mac,
    });
    crate::log::info!(
        "virtio-net: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    crate::drivers::net::register(dev);
    Ok(())
}
