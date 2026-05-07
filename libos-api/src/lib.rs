//! # Hyperion OS API
//!
//! This crate is the **stable public surface** for building things on top of
//! the Hyperion microkernel. It is deliberately small, `#![no_std]`, and
//! has no transitive dependencies on the kernel itself, so it can be:
//!
//! * Pulled into a custom OS / userspace process and used to talk to
//!   Hyperion via syscalls.
//! * Pulled into a host-side build (with default `std` feature) for
//!   docs, tests, and code generation.
//!
//! ## Module map
//!
//! * [`syscall`] — syscall numbers and arg/return ABI types.
//! * [`display`] — generic pixel / monitor / virtual-display types.
//! * [`ui`] — drawing primitives shared with toolkits.
//! * [`fs`] — filesystem-facing types (open flags, errors, …).
//!
//! ## Stability
//!
//! Anything in this crate is committed to backwards compatibility within a
//! major version. The kernel internal types in `hyperion_kernel::*` are
//! **not** stable. If you're writing an OS distribution on top of
//! Hyperion, write to *this* crate.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod display;
pub mod fs;
pub mod syscall;
pub mod ui;

/// Build metadata for whoever is importing this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
