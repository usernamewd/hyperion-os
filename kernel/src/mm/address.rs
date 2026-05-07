//! Strongly-typed physical / virtual addresses.

use core::fmt;

/// Physical address (bus-visible).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

/// Virtual address (CPU-visible after the MMU is on).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

impl PhysAddr {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn raw(self) -> u64 {
        self.0
    }
    pub const fn align_down(self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }
    pub const fn align_up(self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }
}

impl VirtAddr {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn raw(self) -> u64 {
        self.0
    }
    pub const fn as_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PA({:#x})", self.0)
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VA({:#x})", self.0)
    }
}
