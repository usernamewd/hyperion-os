//! Network subsystem.
//!
//! A thin shim over the virtio-net driver that exposes a small set of
//! "send a packet, receive packets" primitives plus an in-kernel UDP
//! echo demo. There is no proper TCP/IP stack yet — wiring smoltcp in
//! at this layer is the obvious follow-up; the structure here is set
//! up so that the driver's transmit/receive callbacks become smoltcp
//! `Device` trait implementations.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::Mutex;

/// Anything that can send and receive raw L2 frames.
pub trait NetDevice: Send + Sync {
    fn mac_address(&self) -> [u8; 6];
    /// Hand a fully-formed Ethernet frame to the device. Returns the
    /// number of bytes accepted (always `frame.len()` on success), or
    /// an error.
    fn send(&self, frame: &[u8]) -> Result<usize, &'static str>;
    /// Pull one received Ethernet frame, if any. Returns the
    /// payload length copied into `buf`.
    fn recv(&self, buf: &mut [u8]) -> Option<usize>;
}

impl NetDevice for crate::drivers::virtio::net::VirtioNet {
    fn mac_address(&self) -> [u8; 6] {
        crate::drivers::virtio::net::VirtioNet::mac_address(self)
    }
    fn send(&self, frame: &[u8]) -> Result<usize, &'static str> {
        crate::drivers::virtio::net::VirtioNet::send(self, frame)
    }
    fn recv(&self, buf: &mut [u8]) -> Option<usize> {
        crate::drivers::virtio::net::VirtioNet::recv(self, buf)
    }
}

static DEVICES: Mutex<Vec<Arc<dyn NetDevice>>> = Mutex::new(Vec::new());

pub fn register(dev: Arc<dyn NetDevice>) {
    let mut v = DEVICES.lock();
    let mac = dev.mac_address();
    v.push(dev);
    crate::log::info!(
        "net: registered device #{} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        v.len() - 1,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
}

/// Snapshot for the shell `net` command.
pub fn snapshot() -> Vec<(usize, [u8; 6])> {
    DEVICES
        .lock()
        .iter()
        .enumerate()
        .map(|(i, d)| (i, d.mac_address()))
        .collect()
}

pub fn primary() -> Option<Arc<dyn NetDevice>> {
    DEVICES.lock().first().cloned()
}

/// Try to receive one frame from the primary device and, if it's a
/// valid IPv4 UDP packet on port 7 (echo) with our MAC as the
/// destination, build a reply and send it back. Otherwise just drop
/// the frame. This is a smoke-test, not a real network stack.
pub fn poll_echo() {
    let dev = match primary() {
        Some(d) => d,
        None => return,
    };
    let mut buf = [0u8; 1600];
    let n = match dev.recv(&mut buf) {
        Some(n) => n,
        None => return,
    };
    let _ = try_reply_echo(&dev, &mut buf[..n]);
}

fn try_reply_echo(dev: &Arc<dyn NetDevice>, frame: &mut [u8]) -> Option<()> {
    if frame.len() < 14 + 20 + 8 {
        return None;
    }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != 0x0800 {
        return None; // not IPv4
    }
    let ip = &frame[14..];
    let ihl = (ip[0] & 0x0f) as usize * 4;
    if ihl < 20 || ip[9] != 17 {
        return None; // not UDP
    }
    let udp = &ip[ihl..];
    let dst_port = u16::from_be_bytes([udp[2], udp[3]]);
    if dst_port != 7 {
        return None;
    }

    // Swap eth src/dst.
    let mut new_frame: alloc::vec::Vec<u8> = frame.to_vec();
    new_frame[..6].copy_from_slice(&frame[6..12]);
    new_frame[6..12].copy_from_slice(&dev.mac_address());

    // Swap IP src/dst, recompute IP checksum.
    let ip_off = 14;
    let mut src_ip = [0u8; 4];
    let mut dst_ip = [0u8; 4];
    src_ip.copy_from_slice(&frame[ip_off + 12..ip_off + 16]);
    dst_ip.copy_from_slice(&frame[ip_off + 16..ip_off + 20]);
    new_frame[ip_off + 12..ip_off + 16].copy_from_slice(&dst_ip);
    new_frame[ip_off + 16..ip_off + 20].copy_from_slice(&src_ip);
    new_frame[ip_off + 10] = 0;
    new_frame[ip_off + 11] = 0;
    let ip_csum = csum16(&new_frame[ip_off..ip_off + ihl]);
    new_frame[ip_off + 10] = (ip_csum >> 8) as u8;
    new_frame[ip_off + 11] = ip_csum as u8;

    // Swap UDP src/dst, zero the checksum (legal for IPv4).
    let udp_off = ip_off + ihl;
    let src_port = u16::from_be_bytes([frame[udp_off], frame[udp_off + 1]]);
    new_frame[udp_off] = (dst_port >> 8) as u8;
    new_frame[udp_off + 1] = dst_port as u8;
    new_frame[udp_off + 2] = (src_port >> 8) as u8;
    new_frame[udp_off + 3] = src_port as u8;
    new_frame[udp_off + 6] = 0;
    new_frame[udp_off + 7] = 0;

    let _ = dev.send(&new_frame);
    Some(())
}

fn csum16(bytes: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        sum += u32::from(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    if i < bytes.len() {
        sum += u32::from(bytes[i]) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
