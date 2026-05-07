//! Framebuffer abstraction.
//!
//! A [`Framebuffer`] is a 2-D pixel buffer with a known width, height,
//! stride and pixel format. It can be backed by either:
//!
//! * a heap-allocated `Vec<u8>` (the default, used by virtual monitors
//!   and offscreen surfaces), or
//! * a fixed MMIO region handed to us by firmware — UEFI GOP, a DTB
//!   `simple-framebuffer`, or a future virtio-gpu scanout buffer.
//!
//! Both backings are accessed through the same `pixels()` /
//! `pixels_mut()` byte-slice interface, so all the drawing primitives
//! work transparently against either one.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Bytes in memory: B, G, R, A. The QEMU virtio-gpu and OVMF GOP
    /// default.
    Bgra8,
    /// Bytes in memory: R, G, B, A. Some U-Boot framebuffers and
    /// certain GOP modes use this.
    Rgba8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => 4,
        }
    }
}

/// Where the pixel bytes live.
enum FbBacking {
    /// Owned heap buffer.
    Heap(Vec<u8>),
    /// Borrowed MMIO region; not freed on drop.
    ///
    /// SAFETY invariant: `base` is valid identity-mapped MMIO of at
    /// least `len` bytes for the lifetime of this `Framebuffer`. We
    /// never read or write outside that range.
    Mmio { base: *mut u8, len: usize },
}

// SAFETY: MMIO regions are accessed under the monitor's `Mutex` so
// there is at most one accessor at a time. The raw pointer is just an
// address handed to us by firmware; it is not tied to a Rust allocation.
unsafe impl Send for FbBacking {}
unsafe impl Sync for FbBacking {}

/// A linear, row-major pixel buffer.
pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    /// Bytes per row. Usually `width * bytes_per_pixel`, but firmware
    /// framebuffers sometimes pad to a hardware-friendly stride.
    pub stride: u32,
    pub format: PixelFormat,
    backing: FbBacking,
}

impl Framebuffer {
    /// Heap-backed framebuffer with stride == width * bpp.
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let stride = width * format.bytes_per_pixel() as u32;
        let pixels = alloc::vec![0u8; (stride * height) as usize];
        Self {
            width,
            height,
            stride,
            format,
            backing: FbBacking::Heap(pixels),
        }
    }

    /// Wrap an MMIO region (e.g. UEFI GOP framebuffer or DTB
    /// `simple-framebuffer`) as a [`Framebuffer`].
    ///
    /// # Safety
    /// `base` must be valid identity-mapped MMIO of at least
    /// `stride * height` bytes for as long as the returned
    /// `Framebuffer` is alive. The region must not alias any other
    /// Rust reference.
    pub unsafe fn from_mmio(
        base: *mut u8,
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
    ) -> Self {
        let len = (stride as usize) * (height as usize);
        Self {
            width,
            height,
            stride,
            format,
            backing: FbBacking::Mmio { base, len },
        }
    }

    /// Read-only view of the pixel bytes.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        match &self.backing {
            FbBacking::Heap(v) => v.as_slice(),
            // SAFETY: invariant on FbBacking::Mmio guarantees `base..base+len`
            // is valid; the slice's lifetime is bounded by `&self`.
            FbBacking::Mmio { base, len } => unsafe { core::slice::from_raw_parts(*base, *len) },
        }
    }

    /// Mutable view of the pixel bytes.
    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        match &mut self.backing {
            FbBacking::Heap(v) => v.as_mut_slice(),
            // SAFETY: see `pixels`. Mutability is fine because we hold
            // `&mut self`.
            FbBacking::Mmio { base, len } => unsafe {
                core::slice::from_raw_parts_mut(*base, *len)
            },
        }
    }

    /// True if the underlying memory is firmware-mapped MMIO that we
    /// don't own.
    pub fn is_mmio(&self) -> bool {
        matches!(self.backing, FbBacking::Mmio { .. })
    }

    /// Encode `(r, g, b, a)` for the buffer's pixel format.
    #[inline]
    pub fn encode_rgba(&self, r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        match self.format {
            PixelFormat::Bgra8 => [b, g, r, a],
            PixelFormat::Rgba8 => [r, g, b, a],
        }
    }

    /// Set a single pixel. Out-of-bounds writes are silently ignored.
    pub fn put_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let off = (y * self.stride + x * 4) as usize;
        let pix = self.encode_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        self.pixels_mut()[off..off + 4].copy_from_slice(&pix);
    }

    /// Fill the entire framebuffer with a single colour.
    pub fn clear(&mut self, rgba: [u8; 4]) {
        let pix = self.encode_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let stride = self.stride as usize;
        let width_bytes = (self.width as usize) * 4;
        let height = self.height as usize;
        let buf = self.pixels_mut();
        for y in 0..height {
            let row_start = y * stride;
            let mut x = 0;
            while x < width_bytes {
                buf[row_start + x..row_start + x + 4].copy_from_slice(&pix);
                x += 4;
            }
        }
    }

    /// Fill an axis-aligned rectangle.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) {
        let pix = self.encode_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let stride = self.stride as usize;
        let buf = self.pixels_mut();
        for yy in y..y_end {
            let row_start = (yy as usize) * stride + (x as usize) * 4;
            for xx in 0..(x_end - x) as usize {
                let off = row_start + xx * 4;
                buf[off..off + 4].copy_from_slice(&pix);
            }
        }
    }

    /// A 1-pixel-wide rectangular outline.
    pub fn stroke_rect(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) {
        if w == 0 || h == 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, rgba);
        self.fill_rect(x, y + h.saturating_sub(1), w, 1, rgba);
        self.fill_rect(x, y, 1, h, rgba);
        self.fill_rect(x + w.saturating_sub(1), y, 1, h, rgba);
    }

    /// Bytes occupied by the pixel buffer.
    pub fn byte_len(&self) -> usize {
        match &self.backing {
            FbBacking::Heap(v) => v.len(),
            FbBacking::Mmio { len, .. } => *len,
        }
    }
}
