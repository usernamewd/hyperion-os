//! Kernel logging.
//!
//! A tiny [`log`]-compatible logger that pipes everything to the PL011
//! UART, serialised by a spinlock. We expose the [`log`] macros via
//! `pub use log::{info,warn,error,debug,trace}` so call sites read
//! identically inside and outside the kernel crate.

use core::fmt::Write;

use spin::Mutex;

pub use log::{debug, error, info, trace, warn};

use crate::arch::aarch64::uart::Uart;

/// Single global UART writer. We hold this for the duration of one log
/// statement to keep multi-line records uninterleaved.
static UART: Mutex<Uart> = Mutex::new(Uart);

struct KernelLogger;

impl ::log::Log for KernelLogger {
    fn enabled(&self, _: &::log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &::log::Record) {
        let mut uart = UART.lock();
        let _ = writeln!(uart, "[{:>5}] {}", record.level(), record.args());
    }

    fn flush(&self) {}
}

static LOGGER: KernelLogger = KernelLogger;

/// Initialise the [`log`] facade. Idempotent.
pub fn init() {
    let _ = ::log::set_logger(&LOGGER);
    ::log::set_max_level(::log::LevelFilter::Info);
}

/// Print to the UART without going through the [`log`] facade. Useful from
/// the panic handler where the logger lock might already be held.
pub fn raw_print(args: core::fmt::Arguments<'_>) {
    // We deliberately use `try_lock` here so we don't deadlock if the
    // panic happens while another CPU holds the logger.
    if let Some(mut uart) = UART.try_lock() {
        let _ = uart.write_fmt(args);
    } else {
        // Fallback: write directly, accepting that output may interleave.
        let mut uart = Uart;
        let _ = uart.write_fmt(args);
    }
}

/// Macro version of [`raw_print`].
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        $crate::log::raw_print(core::format_args!($($arg)*))
    }};
}

/// `kprint!` with a trailing newline.
#[macro_export]
macro_rules! kprintln {
    () => {{
        $crate::log::raw_print(core::format_args!("\r\n"))
    }};
    ($($arg:tt)*) => {{
        $crate::log::raw_print(core::format_args!($($arg)*));
        $crate::log::raw_print(core::format_args!("\r\n"))
    }};
}
