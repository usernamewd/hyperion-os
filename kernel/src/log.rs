//! Kernel logging.
//!
//! Tiny [`log`]-compatible logger that pipes everything to the active
//! boot console (whichever UART the HAL picked at boot — PL011, 16550,
//! BCM mini-UART, …). All writes are serialised by a spinlock so
//! multi-line records stay uninterleaved.

use core::fmt::Write;

use spin::Mutex;

pub use log::{debug, error, info, trace, warn};

use crate::hal::console::Writer;

/// Lock around the console writer. The `Writer` is zero-sized; the
/// actual console handle lives in `crate::hal::console`. We hold this
/// for the duration of one log record so multi-line entries don't get
/// interleaved with concurrent log calls.
static WRITER: Mutex<Writer> = Mutex::new(Writer);

struct KernelLogger;

impl ::log::Log for KernelLogger {
    fn enabled(&self, _: &::log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &::log::Record) {
        let mut w = WRITER.lock();
        let _ = writeln!(w, "[{:>5}] {}", record.level(), record.args());
    }

    fn flush(&self) {}
}

static LOGGER: KernelLogger = KernelLogger;

/// Initialise the [`log`] facade. Idempotent.
pub fn init() {
    let _ = ::log::set_logger(&LOGGER);
    ::log::set_max_level(::log::LevelFilter::Info);
}

/// Print to the console without going through the [`log`] facade. Useful
/// from the panic handler where the logger lock might already be held.
pub fn raw_print(args: core::fmt::Arguments<'_>) {
    // `try_lock` so we don't deadlock if the panic happens while another
    // CPU holds the logger.
    if let Some(mut w) = WRITER.try_lock() {
        let _ = w.write_fmt(args);
    } else {
        // Fallback: write directly, accepting that output may interleave.
        let mut w = Writer;
        let _ = w.write_fmt(args);
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
