//! Build script for the Hyperion kernel.
//!
//! Two responsibilities:
//!
//! 1. Pick the right linker script for the current target arch and
//!    pin it onto the bin link.
//! 2. Pass arch-specific link arguments (e.g. `--no-eh-frame-hdr` on
//!    aarch64; `-z noexecstack` / `-z max-page-size=0x1000` on
//!    x86_64).
//!
//! Why a build script and not `[target.<triple>] rustflags = [...]` in
//! `.cargo/config.toml`? With `[profile.release] lto = "thin"` rustc
//! silently drops `link-arg` rustflags during the final bin link step
//! (https://github.com/rust-lang/cargo/issues/9554), so build.rs is
//! the only reliable hook. Codegen-only flags stay in
//! `.cargo/config.toml`.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let script_name = match target_arch.as_str() {
        "x86_64" => "linker-x86_64.ld",
        // aarch64 + host builds. Host builds never link the kernel
        // binary so the value is unused there.
        _ => "linker.ld",
    };

    let linker_script: PathBuf = [&manifest_dir, script_name].iter().collect();
    let linker_script = linker_script
        .to_str()
        .expect("linker script path is not UTF-8");

    println!("cargo:rustc-link-arg-bins=-T{linker_script}");
    println!("cargo:rustc-link-arg-bins=--no-eh-frame-hdr");

    if target_arch == "x86_64" {
        // GNU ld picks these up; lld accepts them silently.
        println!("cargo:rustc-link-arg-bins=-z");
        println!("cargo:rustc-link-arg-bins=noexecstack");
        println!("cargo:rustc-link-arg-bins=-z");
        println!("cargo:rustc-link-arg-bins=max-page-size=0x1000");
    }

    println!("cargo:rerun-if-changed={linker_script}");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=linker-x86_64.ld");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
}
