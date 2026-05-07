//! Architecture-specific entry points.
//!
//! Hyperion targets aarch64 first; the [`aarch64`] submodule is the only
//! implementation. Adding a new architecture means adding a sibling module
//! and re-exporting its `boot`, `late_init`, `halt` etc. through this
//! module.

pub mod aarch64;
