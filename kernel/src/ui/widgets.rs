//! Tiny widget set demonstrating how to compose [`Canvas`] primitives.
//!
//! These are intentionally minimalist; they're meant to show how a
//! downstream UI toolkit might be structured rather than to compete with
//! one. A widget knows how to render itself onto a Canvas — that's it.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::canvas::Canvas;

/// Anything that can be rendered to a [`Canvas`].
pub trait Widget {
    fn render(&self, c: &mut Canvas);
}

/// A coloured rectangle with optional border + child widgets stacked
/// inside it.
pub struct Panel {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub fill: [u8; 4],
    pub border: Option<[u8; 4]>,
    pub children: Vec<alloc::boxed::Box<dyn Widget>>,
}

impl Panel {
    pub fn new(x: i32, y: i32, w: u32, h: u32, fill: [u8; 4]) -> Self {
        Self {
            x,
            y,
            w,
            h,
            fill,
            border: None,
            children: Vec::new(),
        }
    }

    pub fn with_border(mut self, c: [u8; 4]) -> Self {
        self.border = Some(c);
        self
    }

    pub fn add(&mut self, w: alloc::boxed::Box<dyn Widget>) {
        self.children.push(w);
    }
}

impl Widget for Panel {
    fn render(&self, c: &mut Canvas) {
        c.fill_rect(self.x, self.y, self.w, self.h, self.fill);
        if let Some(b) = self.border {
            c.stroke_rect(self.x, self.y, self.w, self.h, b);
        }
        for child in &self.children {
            child.render(c);
        }
    }
}

/// Plain text label. (x, y) is the top-left corner of the first glyph.
pub struct Label {
    pub x: i32,
    pub y: i32,
    pub text: String,
    pub colour: [u8; 4],
}

impl Label {
    pub fn new(x: i32, y: i32, text: &str, colour: [u8; 4]) -> Self {
        Self {
            x,
            y,
            text: text.to_string(),
            colour,
        }
    }
}

impl Widget for Label {
    fn render(&self, c: &mut Canvas) {
        c.draw_text(self.x, self.y, &self.text, self.colour);
    }
}

/// Static button face — input dispatch is the responsibility of the host
/// toolkit; this widget only owns presentation.
pub struct Button {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub label: String,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
}

impl Button {
    pub fn new(x: i32, y: i32, w: u32, h: u32, label: &str) -> Self {
        Self {
            x,
            y,
            w,
            h,
            label: label.to_string(),
            fg: [0xff, 0xff, 0xff, 0xff],
            bg: [0x33, 0x55, 0xcc, 0xff],
        }
    }
}

impl Widget for Button {
    fn render(&self, c: &mut Canvas) {
        c.fill_rect(self.x, self.y, self.w, self.h, self.bg);
        c.stroke_rect(self.x, self.y, self.w, self.h, [0, 0, 0, 0xff]);
        let tx = self.x + 6;
        let ty = self.y + (self.h as i32 - 8) / 2;
        c.draw_text(tx, ty, &self.label, self.fg);
    }
}
