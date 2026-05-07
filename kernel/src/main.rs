//! Hyperion OS kernel binary.
//!
//! All real work happens in the `hyperion_kernel` library crate; this binary
//! exists only to satisfy the linker. The actual entry point is `_start` in
//! `arch::aarch64::boot`, which is exported by the library and pinned into
//! the `.text.boot` section by the linker script.
//!
//! Splitting kernel logic into a `lib.rs` lets us run host-side unit tests
//! against the platform-independent modules with `cargo test -p hyperion-kernel`
//! once host stubs are wired up.

#![no_std]
#![no_main]

// Pull the library in so its `_start` symbol is preserved by the linker.
#[allow(unused_imports)]
use hyperion_kernel as _;
