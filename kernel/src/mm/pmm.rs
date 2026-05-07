//! Physical Memory Manager.
//!
//! A bitmap allocator over the kernel-managed RAM region. Each bit is one
//! 4 KiB page. The pool starts immediately after the kernel image (after
//! `__kernel_end`) and runs to a configurable upper bound (default 256 MiB
//! of usable RAM, plenty for QEMU's default 512 MiB instance).
//!
//! The allocator is intentionally minimal — alloc / free a single frame
//! at a time. A buddy allocator would replace this without changing
//! callers, but for a microkernel of this scope a bitmap is enough.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::Mutex;

const PAGE_SIZE: usize = 4096;
const MANAGED_BYTES: usize = 256 * 1024 * 1024; // 256 MiB
const NUM_FRAMES: usize = MANAGED_BYTES / PAGE_SIZE;
const BITMAP_WORDS: usize = NUM_FRAMES / 64;

extern "C" {
    static __kernel_end: u8;
}

struct PmmInner {
    bitmap: [u64; BITMAP_WORDS],
    base: usize,
    next_hint: usize,
}

impl PmmInner {
    const fn new() -> Self {
        Self {
            bitmap: [0; BITMAP_WORDS],
            base: 0,
            next_hint: 0,
        }
    }
}

static PMM: Mutex<PmmInner> = Mutex::new(PmmInner::new());
static FREE_FRAMES: AtomicUsize = AtomicUsize::new(0);

/// Initialise the PMM. Must be called exactly once on the boot CPU.
pub fn init() {
    let mut pmm = PMM.lock();
    let kend = (&raw const __kernel_end) as usize;
    let base = (kend + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    pmm.base = base;
    pmm.next_hint = 0;
    for w in pmm.bitmap.iter_mut() {
        *w = 0;
    }
    FREE_FRAMES.store(NUM_FRAMES, Ordering::Relaxed);
}

/// Allocate a single 4 KiB frame, returning its physical (== virtual,
/// while identity-mapped) base address. Returns `None` on OOM.
pub fn alloc_frame() -> Option<usize> {
    let mut pmm = PMM.lock();
    let start = pmm.next_hint;
    for i in 0..NUM_FRAMES {
        let idx = (start + i) % NUM_FRAMES;
        let word = idx / 64;
        let bit = idx % 64;
        if pmm.bitmap[word] & (1u64 << bit) == 0 {
            pmm.bitmap[word] |= 1u64 << bit;
            pmm.next_hint = (idx + 1) % NUM_FRAMES;
            FREE_FRAMES.fetch_sub(1, Ordering::Relaxed);
            return Some(pmm.base + idx * PAGE_SIZE);
        }
    }
    None
}

/// Free a frame previously returned by [`alloc_frame`].
pub fn free_frame(addr: usize) {
    let mut pmm = PMM.lock();
    if addr < pmm.base {
        return;
    }
    let off = addr - pmm.base;
    if off % PAGE_SIZE != 0 {
        return;
    }
    let idx = off / PAGE_SIZE;
    if idx >= NUM_FRAMES {
        return;
    }
    let word = idx / 64;
    let bit = idx % 64;
    if pmm.bitmap[word] & (1u64 << bit) == 0 {
        // Double free; ignore but in a debug build we'd assert.
        return;
    }
    pmm.bitmap[word] &= !(1u64 << bit);
    FREE_FRAMES.fetch_add(1, Ordering::Relaxed);
}

/// Total bytes managed by the PMM.
pub fn total_bytes() -> usize {
    MANAGED_BYTES
}

/// Free bytes currently available.
pub fn free_bytes() -> usize {
    FREE_FRAMES.load(Ordering::Relaxed) * PAGE_SIZE
}
