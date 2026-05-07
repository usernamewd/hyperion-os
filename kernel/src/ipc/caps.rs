//! Capabilities & handle tables.
//!
//! A [`Capability`] is a typed kernel object reference. Threads/processes
//! refer to caps through opaque `Handle`s (small integers, similar to file
//! descriptors). Each cap carries a [`Rights`] bitmask that the kernel
//! checks on every operation, so a process can hand off a *read-only*
//! reference to a port without leaking the ability to send to it.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use bitflags::bitflags;

use super::port::Port;

bitflags! {
    /// Operations a capability authorises.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Rights: u32 {
        const READ      = 1 << 0;
        const WRITE     = 1 << 1;
        const SEND      = 1 << 2;
        const RECEIVE   = 1 << 3;
        const TRANSFER  = 1 << 4;
        const DUPLICATE = 1 << 5;
    }
}

/// Opaque handle into a [`CapTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(pub u32);

/// What a capability points at. New kernel objects (e.g. memory regions)
/// would extend this enum.
#[derive(Clone)]
pub enum Capability {
    Port(Arc<Port>),
}

struct Entry {
    cap: Capability,
    rights: Rights,
}

/// Per-process capability table.
pub struct CapTable {
    next: u32,
    entries: BTreeMap<u32, Entry>,
}

impl CapTable {
    pub const fn new() -> Self {
        Self {
            next: 1,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, cap: Capability, rights: Rights) -> Handle {
        let h = Handle(self.next);
        self.next = self.next.wrapping_add(1);
        self.entries.insert(h.0, Entry { cap, rights });
        h
    }

    pub fn get(&self, h: Handle) -> Option<(&Capability, Rights)> {
        self.entries.get(&h.0).map(|e| (&e.cap, e.rights))
    }

    pub fn remove(&mut self, h: Handle) -> Option<Capability> {
        self.entries.remove(&h.0).map(|e| e.cap)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for CapTable {
    fn default() -> Self {
        Self::new()
    }
}
