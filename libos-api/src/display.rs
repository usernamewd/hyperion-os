//! Display-side types shared with toolkits.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorKind {
    Physical,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorInfo {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub kind: MonitorKind,
    pub format: PixelFormat,
}

/// 32-bit colour helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour(pub [u8; 4]);

impl Colour {
    pub const BLACK: Self = Self([0, 0, 0, 0xff]);
    pub const WHITE: Self = Self([0xff; 4]);
    pub const RED: Self = Self([0xff, 0, 0, 0xff]);
    pub const GREEN: Self = Self([0, 0xff, 0, 0xff]);
    pub const BLUE: Self = Self([0, 0, 0xff, 0xff]);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }
}
