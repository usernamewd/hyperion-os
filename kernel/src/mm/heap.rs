//! Kernel heap allocator.
//!
//! Backed by [`linked_list_allocator`] over the linker-reserved `.heap`
//! region (`__heap_start..__heap_end`). Installed as the `#[global_allocator]`
//! so `alloc::vec::Vec` & friends Just Work in kernel code.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use linked_list_allocator::Heap;
use spin::Mutex;

extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
}

struct LockedHeap {
    inner: Mutex<Heap>,
    used: AtomicUsize,
    total: AtomicUsize,
}

impl LockedHeap {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(Heap::empty()),
            used: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
        }
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.inner.lock();
        match heap.allocate_first_fit(layout) {
            Ok(nn) => {
                self.used.fetch_add(layout.size(), Ordering::Relaxed);
                nn.as_ptr()
            }
            Err(_) => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(nn) = core::ptr::NonNull::new(ptr) {
            // SAFETY: caller obeys the GlobalAlloc contract.
            unsafe {
                self.inner.lock().deallocate(nn, layout);
            }
            self.used.fetch_sub(layout.size(), Ordering::Relaxed);
        }
    }
}

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::new();

/// Initialise the kernel heap. Must be called exactly once.
pub fn init() {
    let start = (&raw const __heap_start) as usize;
    let end = (&raw const __heap_end) as usize;
    let size = end - start;
    HEAP.total.store(size, Ordering::Relaxed);
    // SAFETY: the .heap region is reserved exclusively for this allocator
    // by the linker script.
    unsafe {
        HEAP.inner.lock().init(start as *mut u8, size);
    }
}

pub fn total_bytes() -> usize {
    HEAP.total.load(Ordering::Relaxed)
}

pub fn used_bytes() -> usize {
    HEAP.used.load(Ordering::Relaxed)
}
