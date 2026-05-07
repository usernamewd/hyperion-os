//! Built-in interactive shell.
//!
//! Runs as a kernel thread reading from PL011 UART and writing to the
//! same. It demonstrates how a userland "init" would interact with the
//! microkernel APIs — every command here is implemented in terms of the
//! same public types exposed via [`crate::api`].

mod commands;

use alloc::string::String;
use alloc::vec::Vec;

use crate::arch::aarch64::uart;
use crate::kprintln;

/// Spawn the shell thread.
pub fn spawn() {
    crate::proc::scheduler::spawn("shell", shell_entry, 0);
}

extern "C" fn shell_entry(_arg: usize) -> ! {
    kprintln!();
    kprintln!("Hyperion shell ready. Type 'help' for commands.");
    let mut line = String::new();
    let mut history: Vec<String> = Vec::new();
    loop {
        prompt();
        line.clear();
        read_line(&mut line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        history.push(String::from(trimmed));
        commands::run(trimmed, &history);
    }
}

fn prompt() {
    kprintln!();
    crate::kprint!("hyperion:/ $ ");
}

/// Block-read a line from UART with rudimentary editing (backspace).
fn read_line(buf: &mut String) {
    loop {
        let b = uart::getb_blocking();
        match b {
            b'\r' | b'\n' => {
                uart::putb(b'\r');
                uart::putb(b'\n');
                return;
            }
            0x7f | 0x08 => {
                if buf.pop().is_some() {
                    // erase last char on terminal
                    uart::putb(0x08);
                    uart::putb(b' ');
                    uart::putb(0x08);
                }
            }
            b if (0x20..=0x7e).contains(&b) => {
                buf.push(b as char);
                uart::putb(b);
            }
            _ => {}
        }
    }
}
