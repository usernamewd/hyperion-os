//! Tiny stacking compositor.
//!
//! The compositor takes a list of [`Layer`]s (each one a [`Framebuffer`]
//! plus a `(x, y, z)` placement) and blits them onto a target monitor in
//! z-order. Alpha is currently treated as binary (0 = transparent,
//! everything else = opaque) to keep this CPU-only path cheap.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::framebuffer::Framebuffer;
use super::monitor::Monitor;
use crate::sync::Mutex;

#[derive(Clone)]
pub struct Layer {
    pub fb: Arc<Mutex<Framebuffer>>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Stacking compositor for a single monitor.
pub struct Compositor {
    pub monitor: Arc<Monitor>,
    layers: Mutex<Vec<Layer>>,
    background: [u8; 4],
}

impl Compositor {
    pub fn new(monitor: Arc<Monitor>) -> Self {
        Self {
            monitor,
            layers: Mutex::new(Vec::new()),
            background: [0x10, 0x18, 0x24, 0xff],
        }
    }

    pub fn set_background(&mut self, rgba: [u8; 4]) {
        self.background = rgba;
    }

    pub fn push_layer(&self, layer: Layer) {
        self.layers.lock().push(layer);
    }

    pub fn clear_layers(&self) {
        self.layers.lock().clear();
    }

    /// Recompose all layers onto the monitor.
    pub fn render(&self) {
        // Clone & sort layers under the lock so we don't hold it during
        // the blit.
        let mut layers = self.layers.lock().clone();
        layers.sort_by_key(|l| l.z);

        self.monitor.with_framebuffer(|target| {
            target.clear(self.background);
            for layer in &layers {
                let src = layer.fb.lock();
                blit(target, &src, layer.x, layer.y);
            }
        });
    }
}

fn blit(dst: &mut Framebuffer, src: &Framebuffer, x: i32, y: i32) {
    let src_w = src.width;
    let src_h = src.height;
    let src_stride = src.stride;
    let dst_stride = dst.stride;
    let dst_w = dst.width;
    let dst_h = dst.height;
    let src_bytes = src.pixels();
    let dst_bytes = dst.pixels_mut();
    for sy in 0..src_h {
        let dy = y + sy as i32;
        if dy < 0 || dy as u32 >= dst_h {
            continue;
        }
        for sx in 0..src_w {
            let dx = x + sx as i32;
            if dx < 0 || dx as u32 >= dst_w {
                continue;
            }
            let so = (sy * src_stride + sx * 4) as usize;
            let do_ = (dy as u32 * dst_stride + dx as u32 * 4) as usize;
            // Treat alpha=0 pixels as transparent.
            if src_bytes[so + 3] == 0 {
                continue;
            }
            dst_bytes[do_..do_ + 4].copy_from_slice(&src_bytes[so..so + 4]);
        }
    }
}
