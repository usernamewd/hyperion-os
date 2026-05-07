//! Synchronous message ports.
//!
//! A [`Port`] is a small bounded queue of [`Message`]s. Senders block when
//! the queue is full; receivers block when it's empty. This is enough to
//! build remote-procedure-call style services in userland.
//!
//! All bookkeeping is protected by a `Mutex`; this single CPU's scheduler
//! will yield while waiting, so contention isn't catastrophic.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::Mutex;

const PORT_CAPACITY: usize = 32;
const MSG_PAYLOAD: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortError {
    Full,
    Empty,
    Closed,
}

/// Fixed-size message: 8 64-bit slots + a 64-byte inline payload.
#[derive(Clone, Debug)]
pub struct Message {
    pub tag: u64,
    pub words: [u64; 8],
    pub payload: [u8; MSG_PAYLOAD],
    pub payload_len: u8,
}

impl Message {
    pub fn new(tag: u64) -> Self {
        Self {
            tag,
            words: [0; 8],
            payload: [0; MSG_PAYLOAD],
            payload_len: 0,
        }
    }

    pub fn with_payload(tag: u64, bytes: &[u8]) -> Self {
        let mut m = Self::new(tag);
        let n = bytes.len().min(MSG_PAYLOAD);
        m.payload[..n].copy_from_slice(&bytes[..n]);
        m.payload_len = n as u8;
        m
    }
}

struct Inner {
    queue: VecDeque<Message>,
    closed: bool,
}

pub struct Port {
    pub id: u64,
    pub name: String,
    inner: Mutex<Inner>,
}

impl Port {
    pub fn new(name: &str) -> Arc<Self> {
        Arc::new(Self {
            id: alloc_port_id(),
            name: String::from(name),
            inner: Mutex::new(Inner {
                queue: VecDeque::with_capacity(PORT_CAPACITY),
                closed: false,
            }),
        })
    }

    pub fn try_send(&self, msg: Message) -> Result<(), PortError> {
        let mut g = self.inner.lock();
        if g.closed {
            return Err(PortError::Closed);
        }
        if g.queue.len() >= PORT_CAPACITY {
            return Err(PortError::Full);
        }
        g.queue.push_back(msg);
        Ok(())
    }

    pub fn try_recv(&self) -> Result<Message, PortError> {
        let mut g = self.inner.lock();
        if let Some(m) = g.queue.pop_front() {
            Ok(m)
        } else if g.closed {
            Err(PortError::Closed)
        } else {
            Err(PortError::Empty)
        }
    }

    /// Block until a message is available, yielding the CPU between polls.
    pub fn recv(&self) -> Result<Message, PortError> {
        loop {
            match self.try_recv() {
                Ok(m) => return Ok(m),
                Err(PortError::Empty) => crate::proc::scheduler::yield_now(),
                Err(e) => return Err(e),
            }
        }
    }

    /// Try to send; if the queue is full, yield once and try again. The
    /// caller can keep calling this in a loop for full blocking semantics.
    pub fn send(&self, msg: Message) -> Result<(), PortError> {
        match self.try_send(msg) {
            Ok(()) => Ok(()),
            Err(PortError::Full) => {
                crate::proc::scheduler::yield_now();
                Err(PortError::Full)
            }
            Err(e) => Err(e),
        }
    }

    pub fn close(&self) {
        self.inner.lock().closed = true;
    }

    pub fn pending(&self) -> usize {
        self.inner.lock().queue.len()
    }
}

static NEXT_PORT_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_port_id() -> u64 {
    NEXT_PORT_ID.fetch_add(1, Ordering::Relaxed)
}

/// One-time IPC init. Currently a placeholder for future global tables.
pub fn init() {}
