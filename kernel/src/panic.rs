//! Panic handler.
//!
//! On a kernel panic we want to (a) print a clear message including the
//! panic location and message, and (b) come to a graceful stop without
//! taking the system into a wedged state. We mask IRQs, dump the panic
//! info to the UART, then halt the CPU.

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    // Mask interrupts immediately so we don't get rescheduled mid-print.
    crate::arch::disable_irqs();

    crate::kprintln!(
        "\r\n!!! KERNEL PANIC !!!\r\n  at {}\r\n  message: {}\r\n",
        info.location()
            .map(|l| l as &dyn core::fmt::Display)
            .unwrap_or(&"<unknown>"),
        info.message(),
    );

    crate::arch::halt();
}
