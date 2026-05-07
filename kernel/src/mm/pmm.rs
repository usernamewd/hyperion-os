//! Physical Memory Manager.
//!
//! Bitmap allocator over the largest RAM bank reported by the HAL. Each
//! bit covers one 4 KiB page. The pool starts at the page immediately
//! after the kernel image (`__kernel_end`, when that lies inside the
//! bank) and runs for as many frames as the bitmap can describe — up to
//! `MAX_MANAGED_BYTES` (256 MiB by default, comfortably more than QEMU
//! virt's 512 MiB instance with the kernel image carved out).
//!
//! The allocator is intentionally minimal — alloc / free a single frame
//! at a time. Once we want NUMA awareness or a buddy allocator the API
//! stays the same; only this module changes.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::Mutex;

const PAGE_SIZE: usize = 4096;
/// Hard cap on the bitmap size so the static is bounded. The kernel
/// happily runs with much smaller banks; this just sets the maximum
/// portion of one bank we're prepared to hand out as page frames.
const MAX_MANAGED_BYTES: usize = 256 * 1024 * 1024;
const MAX_FRAMES: usize = MAX_MANAGED_BYTES / PAGE_SIZE;
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

extern "C" {
    static __kernel_end: u8;
}

struct PmmInner {
    bitmap: [u64; BITMAP_WORDS],
    base: usize,
    /// Number of frames actually managed in this run. Always <= MAX_FRAMES.
    num_frames: usize,
    next_hint: usize,
}

impl PmmInner {
    const fn new() -> Self {
        Self {
            bitmap: [0; BITMAP_WORDS],
            base: 0,
            num_frames: 0,
            next_hint: 0,
        }
    }
}

static PMM: Mutex<PmmInner> = Mutex::new(PmmInner::new());
static FREE_FRAMES: AtomicUsize = AtomicUsize::new(0);
static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Initialise the PMM. Must be called exactly once on the boot CPU,
/// after [`crate::hal::init`] has published a [`crate::hal::BootInfo`].
pub fn init() {
    // Pick the largest contiguous RAM bank the HAL discovered. If
    // `__kernel_end` falls inside it, start the pool one page after.
    let bi = crate::hal::info();
    let bank = match bi.memory.largest() {
        Some(b) => b,
        None => return,
    };
    let kend = (&raw const __kernel_end) as usize as u64;
    let bank_start = bank.base;
    let bank_end = bank.base.saturating_add(bank.size);
    let raw_base = if (bank_start..bank_end).contains(&kend) {
        kend
    } else {
        bank_start
    };
    let base = ((raw_base as usize) + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let usable_bytes = (bank_end as usize)
        .saturating_sub(base)
        .min(MAX_MANAGED_BYTES);
    let num_frames = (usable_bytes / PAGE_SIZE).min(MAX_FRAMES);

    let mut pmm = PMM.lock();
    pmm.base = base;
    pmm.num_frames = num_frames;
    pmm.next_hint = 0;
    for w in pmm.bitmap.iter_mut() {
        *w = 0;
    }
    FREE_FRAMES.store(num_frames, Ordering::Relaxed);
    TOTAL_BYTES.store(num_frames * PAGE_SIZE, Ordering::Relaxed);
}

/// Allocate a single 4 KiB frame, returning its physical (== virtual,
/// while identity-mapped) base address. Returns `None` on OOM.
pub fn alloc_frame() -> Option<usize> {
    let mut pmm = PMM.lock();
    let n = pmm.num_frames;
    if n == 0 {
        return None;
    }
    let start = pmm.next_hint;
    for i in 0..n {
        let idx = (start + i) % n;
        let word = idx / 64;
        let bit = idx % 64;
        if pmm.bitmap[word] & (1u64 << bit) == 0 {
            pmm.bitmap[word] |= 1u64 << bit;
            pmm.next_hint = (idx + 1) % n;
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
    if idx >= pmm.num_frames {
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
    TOTAL_BYTES.load(Ordering::Relaxed)
}

/// Free bytes currently available.
pub fn free_bytes() -> usize {
    FREE_FRAMES.load(Ordering::Relaxed) * PAGE_SIZE
}
