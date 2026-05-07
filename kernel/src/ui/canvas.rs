//! Canvas: high-level drawing on top of a [`crate::display::Framebuffer`].

use crate::display::framebuffer::Framebuffer;
use crate::ui::font::Font8x8;

/// Stateless wrapper that gives a framebuffer a richer drawing API.
/// Holding a `Canvas` borrows the framebuffer mutably.
pub struct Canvas<'a> {
    fb: &'a mut Framebuffer,
}

impl<'a> Canvas<'a> {
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self { fb }
    }

    pub fn width(&self) -> u32 {
        self.fb.width
    }
    pub fn height(&self) -> u32 {
        self.fb.height
    }

    pub fn clear(&mut self, rgba: [u8; 4]) {
        self.fb.clear(rgba);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, rgba: [u8; 4]) {
        let (x, y, w, h) = clip(x, y, w, h, self.fb.width, self.fb.height);
        self.fb.fill_rect(x, y, w, h, rgba);
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: u32, h: u32, rgba: [u8; 4]) {
        let (x, y, w, h) = clip(x, y, w, h, self.fb.width, self.fb.height);
        self.fb.stroke_rect(x, y, w, h, rgba);
    }

    /// Bresenham line.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgba: [u8; 4]) {
        let (mut x0, mut y0) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if x0 >= 0 && y0 >= 0 {
                self.fb.put_pixel(x0 as u32, y0 as u32, rgba);
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Render `text` at `(x, y)` with the built-in 8x8 font.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, rgba: [u8; 4]) {
        let font = Font8x8;
        for (i, ch) in text.chars().enumerate() {
            let cx = x + (i as i32) * 8;
            font.draw_char(self.fb, cx, y, ch, rgba);
        }
    }
}

fn clip(x: i32, y: i32, w: u32, h: u32, sw: u32, sh: u32) -> (u32, u32, u32, u32) {
    let nx = x.max(0) as u32;
    let ny = y.max(0) as u32;
    if nx >= sw || ny >= sh {
        return (0, 0, 0, 0);
    }
    let mw = (w as i32 + x.min(0)).max(0) as u32;
    let mh = (h as i32 + y.min(0)).max(0) as u32;
    let nw = mw.min(sw - nx);
    let nh = mh.min(sh - ny);
    (nx, ny, nw, nh)
}
