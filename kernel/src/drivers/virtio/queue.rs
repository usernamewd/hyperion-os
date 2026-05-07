//! Virtio split virtqueue (single-queue, packed-ring not supported).
//!
//! Layout per virtio 1.1 §2.6:
//!
//! * Descriptor table — `[Desc; size]`, 16 bytes each.
//! * Available ring — header + `size` u16 entries + used-event u16.
//! * Used ring — header + `size` UsedElem entries + avail-event u16.
//!
//! This is the "split" virtqueue layout that every device since virtio
//! 0.95 has supported. We allocate all three rings inside one
//! page-aligned heap allocation so we can hand the device a single
//! contiguous physical address per ring (the kernel runs identity-
//! mapped, so virtual address == physical address).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{fence, Ordering};

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy)]
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

#[repr(C)]
#[derive(Debug)]
pub struct AvailRing {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; 0],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct UsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct UsedRing {
    pub flags: u16,
    pub idx: u16,
    pub ring: [UsedElem; 0],
}

/// A heap-allocated split virtqueue.
pub struct Virtqueue {
    pub size: u16,
    pub desc: Box<[Desc]>,
    /// Backing store for the available ring (flags + idx + ring + event).
    avail: Vec<u8>,
    /// Backing store for the used ring (flags + idx + ring + event).
    used: Vec<u8>,
    pub free_head: u16,
    pub num_free: u16,
    pub last_used_idx: u16,
}

impl Virtqueue {
    pub fn new(size: u16) -> Self {
        assert!(size.is_power_of_two() && size > 0);
        let mut desc: Box<[Desc]> = vec![Desc::default(); size as usize].into_boxed_slice();
        // Chain free descriptors so the ring starts as one big free list.
        for i in 0..size {
            desc[i as usize].next = if i + 1 < size { i + 1 } else { 0 };
            desc[i as usize].flags = VRING_DESC_F_NEXT;
        }
        let avail_len = 4 + 2 * size as usize + 2;
        let used_len = 4 + 8 * size as usize + 2;
        Self {
            size,
            desc,
            avail: vec![0u8; avail_len],
            used: vec![0u8; used_len],
            free_head: 0,
            num_free: size,
            last_used_idx: 0,
        }
    }

    pub fn desc_phys(&self) -> u64 {
        self.desc.as_ptr() as u64
    }

    pub fn avail_phys(&self) -> u64 {
        self.avail.as_ptr() as u64
    }

    pub fn used_phys(&self) -> u64 {
        self.used.as_ptr() as u64
    }

    /// Allocate one descriptor from the free list. Returns the index or
    /// `None` if exhausted.
    pub fn alloc_desc(&mut self) -> Option<u16> {
        if self.num_free == 0 {
            return None;
        }
        let idx = self.free_head;
        self.free_head = self.desc[idx as usize].next;
        self.num_free -= 1;
        let d = &mut self.desc[idx as usize];
        d.addr = 0;
        d.len = 0;
        d.flags = 0;
        d.next = 0;
        Some(idx)
    }

    /// Allocate `n` chained descriptors and return the head index. The
    /// descriptors are linked through `next` with `VRING_DESC_F_NEXT`.
    pub fn alloc_chain(&mut self, n: usize) -> Option<u16> {
        if (n as u16) > self.num_free || n == 0 {
            return None;
        }
        let head = self.alloc_desc()?;
        let mut prev = head;
        for _ in 1..n {
            let next = self.alloc_desc()?;
            self.desc[prev as usize].flags |= VRING_DESC_F_NEXT;
            self.desc[prev as usize].next = next;
            prev = next;
        }
        Some(head)
    }

    /// Return descriptors `head..` back to the free list.
    pub fn free_chain(&mut self, head: u16) {
        let mut idx = head;
        loop {
            let next = self.desc[idx as usize].next;
            let has_next = self.desc[idx as usize].flags & VRING_DESC_F_NEXT != 0;
            self.desc[idx as usize].flags = VRING_DESC_F_NEXT;
            self.desc[idx as usize].next = self.free_head;
            self.free_head = idx;
            self.num_free += 1;
            if !has_next {
                break;
            }
            idx = next;
        }
    }

    /// Push descriptor `head` into the available ring and bump the idx.
    pub fn push_avail(&mut self, head: u16) {
        let size = self.size as usize;
        // Layout: [flags:u16][idx:u16][ring: [u16; size]][event:u16]
        let avail_idx = u16::from_le_bytes([self.avail[2], self.avail[3]]);
        let slot = (avail_idx as usize) & (size - 1);
        let off = 4 + slot * 2;
        let bytes = head.to_le_bytes();
        self.avail[off] = bytes[0];
        self.avail[off + 1] = bytes[1];
        fence(Ordering::SeqCst);
        let new_idx = avail_idx.wrapping_add(1);
        let nb = new_idx.to_le_bytes();
        self.avail[2] = nb[0];
        self.avail[3] = nb[1];
        fence(Ordering::SeqCst);
    }

    /// Returns `Some((id, len))` if the device has produced a new used
    /// element since our last call, advancing `last_used_idx`.
    pub fn pop_used(&mut self) -> Option<(u32, u32)> {
        let device_idx = u16::from_le_bytes([self.used[2], self.used[3]]);
        if device_idx == self.last_used_idx {
            return None;
        }
        let size = self.size as usize;
        let slot = (self.last_used_idx as usize) & (size - 1);
        let off = 4 + slot * 8;
        let id = u32::from_le_bytes([
            self.used[off],
            self.used[off + 1],
            self.used[off + 2],
            self.used[off + 3],
        ]);
        let len = u32::from_le_bytes([
            self.used[off + 4],
            self.used[off + 5],
            self.used[off + 6],
            self.used[off + 7],
        ]);
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Some((id, len))
    }

    /// Spin-wait for the next used element (suitable for boot-time
    /// driver init only; real I/O uses interrupts via the queue's
    /// notification mechanism).
    pub fn wait_used(&mut self) -> (u32, u32) {
        loop {
            if let Some(elem) = self.pop_used() {
                return elem;
            }
            core::hint::spin_loop();
        }
    }
}
