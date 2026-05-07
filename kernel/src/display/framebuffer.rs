//! Framebuffer abstraction.
//!
//! A [`Framebuffer`] is just a heap-allocated pixel buffer with a known
//! width, height, stride and pixel format. It is the common substrate for
//! both monitors and offscreen surfaces used by the compositor.
//!
//! Pixel format helpers live alongside; we standardise on 32-bit BGRA in
//! little-endian (`B,G,R,A` in memory) because that's what virtio-gpu and
//! the QEMU `bochs-display` device expect by default.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => 4,
        }
    }
}

/// A linear, row-major pixel buffer.
pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub pixels: Vec<u8>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let stride = width * format.bytes_per_pixel() as u32;
        let pixels = alloc::vec![0u8; (stride * height) as usize];
        Self {
            width,
            height,
            stride,
            format,
            pixels,
        }
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
        self.pixels[off..off + 4].copy_from_slice(&pix);
    }

    /// Fill the entire framebuffer with a single colour.
    pub fn clear(&mut self, rgba: [u8; 4]) {
        let pix = self.encode_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&pix);
        }
    }

    /// Fill an axis-aligned rectangle.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) {
        let pix = self.encode_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for yy in y..y_end {
            let row_start = (yy * self.stride + x * 4) as usize;
            for xx in 0..(x_end - x) as usize {
                let off = row_start + xx * 4;
                self.pixels[off..off + 4].copy_from_slice(&pix);
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
        self.pixels.len()
    }
}
