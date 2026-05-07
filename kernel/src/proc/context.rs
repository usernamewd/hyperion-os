//! Cross-arch facade over the architecture-specific thread context
//! switch. The actual `Context` struct, `__ctx_switch` symbol, and
//! `init_context` helper live under `crate::arch::<arch>::context`.

#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::context::{init_context, Context, __ctx_switch};

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::context::{init_context, Context, __ctx_switch};
