//! Build script for the Hyperion kernel.
//!
//! The kernel must be linked with our custom `linker.ld`. Passing the
//! linker script via `.cargo/config.toml` `rustflags = ["-C", "link-arg=..."]`
//! works for the lib build but is silently dropped by rustc when the bin
//! crate is linked with thin LTO + linker-plugin-lto (which is what cargo
//! turns on for `[profile.release] lto = "thin"` plus dependencies built
//! with metadata). The reliable way to attach a linker script to the
//! *final* bin link is to emit `cargo:rustc-link-arg-bins` from build.rs
//! using an absolute path.
//!
//! See <https://github.com/rust-lang/cargo/issues/9554>.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let linker_script: PathBuf = [&manifest_dir, "linker.ld"].iter().collect();
    let linker_script = linker_script.to_str().expect("linker.ld path is not UTF-8");

    // Apply only when building the kernel binary (not host-side tests of the
    // library). `rustc-link-arg-bins` is the canonical hook for this.
    println!("cargo:rustc-link-arg-bins=-T{linker_script}");
    println!("cargo:rustc-link-arg-bins=--no-eh-frame-hdr");

    // Re-run if the linker script changes.
    println!("cargo:rerun-if-changed={linker_script}");
    // Also re-run if this build script itself changes.
    println!("cargo:rerun-if-changed=build.rs");
}
