//! Inter-process communication.
//!
//! The microkernel itself only does two IPC things: it routes capability
//! handles between address spaces, and it carries fixed-size messages on
//! synchronous **ports**. Everything else (file servers, drivers, display
//! servers) lives outside the kernel and uses these primitives to talk to
//! each other.

pub mod caps;
pub mod port;

pub use caps::{CapTable, Capability, Handle, Rights};
pub use port::{Message, Port, PortError};

/// One-time IPC subsystem init.
pub fn init() {
    port::init();
}
