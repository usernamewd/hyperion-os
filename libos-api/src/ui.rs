//! UI building blocks shared between in-tree widgets and out-of-tree
//! toolkits. Stays trait-light: just enough to express common geometry
//! without forcing a particular widget model.

use crate::display::Colour;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && (x as i64) < (self.x as i64 + self.w as i64)
            && (y as i64) < (self.y as i64 + self.h as i64)
    }
}

/// What a Canvas-backed renderer can do. Implementations live in the
/// kernel's `ui::canvas` (in-process) or in a userland blitter that
/// forwards to the kernel through IPC.
pub trait CanvasOps {
    fn clear(&mut self, c: Colour);
    fn fill_rect(&mut self, r: Rect, c: Colour);
    fn stroke_rect(&mut self, r: Rect, c: Colour);
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Colour);
    fn text(&mut self, x: i32, y: i32, s: &str, c: Colour);
}
