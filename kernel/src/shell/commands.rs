//! Shell commands.
//!
//! Each command is a `fn(args: &[&str])`. Adding a new command is a matter
//! of dropping a row into [`COMMANDS`].

use alloc::string::String;
use alloc::vec::Vec;

use crate::display;
use crate::fs::{vfs, OpenFlags, ROOT};
use crate::kprintln;

type Handler = fn(&[&str], &[String]);

struct Command {
    pub name: &'static str,
    pub help: &'static str,
    pub run: Handler,
}

static COMMANDS: &[Command] = &[
    Command {
        name: "help",
        help: "show this help",
        run: cmd_help,
    },
    Command {
        name: "echo",
        help: "echo arguments to the console",
        run: cmd_echo,
    },
    Command {
        name: "clear",
        help: "clear the screen (ANSI)",
        run: cmd_clear,
    },
    Command {
        name: "ls",
        help: "list a directory (default /)",
        run: cmd_ls,
    },
    Command {
        name: "cat",
        help: "print a file's contents",
        run: cmd_cat,
    },
    Command {
        name: "write",
        help: "write text to a file: write <path> <text...>",
        run: cmd_write,
    },
    Command {
        name: "rm",
        help: "remove a file or empty directory",
        run: cmd_rm,
    },
    Command {
        name: "mkdir",
        help: "create a directory",
        run: cmd_mkdir,
    },
    Command {
        name: "ps",
        help: "list threads/processes",
        run: cmd_ps,
    },
    Command {
        name: "mem",
        help: "show memory stats",
        run: cmd_mem,
    },
    Command {
        name: "display",
        help: "list registered monitors",
        run: cmd_display,
    },
    Command {
        name: "demo",
        help: "render a demo UI to monitor 0",
        run: cmd_demo,
    },
    Command {
        name: "uptime",
        help: "show milliseconds since boot",
        run: cmd_uptime,
    },
    Command {
        name: "history",
        help: "show command history",
        run: cmd_history,
    },
    Command {
        name: "reboot",
        help: "reset the machine",
        run: cmd_reboot,
    },
    Command {
        name: "shutdown",
        help: "power off the machine",
        run: cmd_shutdown,
    },
    Command {
        name: "smp",
        help: "show online CPUs",
        run: cmd_smp,
    },
    Command {
        name: "blk",
        help: "list registered block devices",
        run: cmd_blk,
    },
    Command {
        name: "net",
        help: "list registered net devices / poll for echo",
        run: cmd_net,
    },
    Command {
        name: "gpu",
        help: "draw a colour bar on the virtio-gpu monitor and flush",
        run: cmd_gpu,
    },
    #[cfg(target_arch = "aarch64")]
    Command {
        name: "user",
        help: "drop a thread to EL0 and exercise the SVC dispatcher",
        run: cmd_user,
    },
];

/// Run a single line. Splits on whitespace; dispatches to a [`Command`] or
/// reports "command not found".
pub fn run(line: &str, history: &[String]) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let name = parts[0];
    let args: &[&str] = &parts[1..];
    for c in COMMANDS {
        if c.name == name {
            (c.run)(args, history);
            return;
        }
    }
    kprintln!("hyperion: '{}': command not found", name);
}

fn cmd_help(_args: &[&str], _hist: &[String]) {
    kprintln!("Built-in commands:");
    for c in COMMANDS {
        kprintln!("  {:<10} - {}", c.name, c.help);
    }
}

fn cmd_echo(args: &[&str], _hist: &[String]) {
    kprintln!("{}", args.join(" "));
}

fn cmd_clear(_args: &[&str], _hist: &[String]) {
    crate::kprint!("\x1b[2J\x1b[H");
}

fn cmd_ls(args: &[&str], _hist: &[String]) {
    let path = args.first().copied().unwrap_or("/");
    match ROOT.resolve(path) {
        Ok(node) => {
            let entries = node.list();
            if entries.is_empty() {
                kprintln!("(empty)");
            } else {
                for name in entries {
                    if let Some(child) = node.lookup(&name) {
                        let suffix = if child.ftype() == crate::fs::FileType::Directory {
                            "/"
                        } else {
                            ""
                        };
                        kprintln!("  {}{}", name, suffix);
                    }
                }
            }
        }
        Err(e) => kprintln!("ls: {:?}", e),
    }
}

fn cmd_cat(args: &[&str], _hist: &[String]) {
    let Some(path) = args.first() else {
        kprintln!("usage: cat <path>");
        return;
    };
    match vfs::read_to_string(path) {
        Ok(s) => {
            for line in s.lines() {
                kprintln!("{}", line);
            }
        }
        Err(e) => kprintln!("cat: {:?}", e),
    }
}

fn cmd_write(args: &[&str], _hist: &[String]) {
    if args.len() < 2 {
        kprintln!("usage: write <path> <text...>");
        return;
    }
    let path = args[0];
    let text = args[1..].join(" ");
    let mut f = match ROOT.open(path, OpenFlags::WRITE | OpenFlags::CREATE) {
        Ok(f) => f,
        Err(e) => {
            kprintln!("write: {:?}", e);
            return;
        }
    };
    match f.write(text.as_bytes()) {
        Ok(n) => kprintln!("wrote {} bytes", n),
        Err(e) => kprintln!("write: {:?}", e),
    }
}

fn cmd_rm(args: &[&str], _hist: &[String]) {
    let Some(path) = args.first() else {
        kprintln!("usage: rm <path>");
        return;
    };
    match ROOT.remove(path) {
        Ok(()) => kprintln!("removed {}", path),
        Err(e) => kprintln!("rm: {:?}", e),
    }
}

fn cmd_mkdir(args: &[&str], _hist: &[String]) {
    let Some(path) = args.first() else {
        kprintln!("usage: mkdir <path>");
        return;
    };
    let (parent, name) = match path.rsplit_once('/') {
        Some(("", n)) => ("/", n),
        Some((p, n)) => (p, n),
        None => ("/", *path),
    };
    match ROOT.resolve(parent) {
        Ok(p) => match p.create_dir(name) {
            Ok(_) => kprintln!("created {}", path),
            Err(e) => kprintln!("mkdir: {:?}", e),
        },
        Err(e) => kprintln!("mkdir: {:?}", e),
    }
}

fn cmd_ps(_args: &[&str], _hist: &[String]) {
    kprintln!("PROCESSES:");
    for (pid, name) in crate::proc::process::list() {
        kprintln!("  pid={:<4} {}", pid, name);
    }
    kprintln!("THREADS:");
    for (tid, name, state) in crate::proc::scheduler::snapshot() {
        kprintln!("  tid={:<4} {:<10} {:?}", tid, name, state);
    }
}

fn cmd_mem(_args: &[&str], _hist: &[String]) {
    let s = crate::mm::stats();
    kprintln!(
        "RAM (managed): {} KiB total / {} KiB free",
        s.total_bytes / 1024,
        s.free_bytes / 1024
    );
    kprintln!(
        "Heap:          {} KiB total / {} KiB used",
        s.heap_total / 1024,
        s.heap_used / 1024
    );
}

fn cmd_display(_args: &[&str], _hist: &[String]) {
    let monitors = display::list();
    if monitors.is_empty() {
        kprintln!("(no monitors registered)");
        return;
    }
    for m in monitors {
        kprintln!(
            "  monitor #{:<2} {:<10} {:?} {}x{}",
            m.id().0,
            m.name,
            m.kind,
            m.width,
            m.height
        );
    }
}

fn cmd_demo(_args: &[&str], _hist: &[String]) {
    let Some(mon) = display::get(display::MonitorId(0)) else {
        kprintln!("no monitor 0 registered");
        return;
    };
    mon.with_framebuffer(|fb| {
        let mut canvas = crate::ui::Canvas::new(fb);
        canvas.clear([0x10, 0x18, 0x24, 0xff]);
        canvas.fill_rect(40, 40, 200, 80, [0x33, 0x55, 0xcc, 0xff]);
        canvas.stroke_rect(40, 40, 200, 80, [0xff, 0xff, 0xff, 0xff]);
        canvas.draw_text(60, 70, "Hyperion UI demo", [0xff, 0xff, 0xff, 0xff]);
        canvas.line(
            0,
            0,
            canvas.width() as i32 - 1,
            canvas.height() as i32 - 1,
            [0xff, 0x44, 0x44, 0xff],
        );
    });
    kprintln!(
        "rendered demo to monitor 0 ({}x{} virtual fb)",
        mon.width,
        mon.height
    );
}

fn cmd_uptime(_args: &[&str], _hist: &[String]) {
    let f = crate::arch::timer_freq();
    let c = crate::arch::timer_count();
    let ms = if f == 0 { 0 } else { (c * 1000) / f };
    kprintln!("up {} ms ({} ticks at {} Hz)", ms, c, f);
}

fn cmd_history(_args: &[&str], hist: &[String]) {
    for (i, line) in hist.iter().enumerate() {
        kprintln!("  {:>3}  {}", i + 1, line);
    }
}

fn cmd_reboot(_args: &[&str], _hist: &[String]) {
    kprintln!("reboot requested...");
    crate::arch::system_reset();
}

fn cmd_shutdown(_args: &[&str], _hist: &[String]) {
    kprintln!("shutdown requested...");
    crate::arch::system_off();
}

fn cmd_smp(_args: &[&str], _hist: &[String]) {
    use crate::proc::percpu;
    let cur = percpu::current_cpu_id();
    let online = percpu::online_cpus();
    kprintln!(
        "boot cpu = {}, online count = {}, mask = {:#018b}",
        cur,
        percpu::online_count(),
        percpu::online_mask()
    );
    for c in online {
        kprintln!("  cpu{} online", c);
    }
}

fn cmd_blk(_args: &[&str], _hist: &[String]) {
    let devs = crate::drivers::block::snapshot();
    if devs.is_empty() {
        kprintln!("(no block devices)");
        return;
    }
    for (i, sectors) in devs {
        kprintln!(
            "  block#{:<2} sectors={} ({} KiB)",
            i,
            sectors,
            sectors / 2
        );
    }
    kprintln!("Try `cat /blk/info` and `cat /blk/sector0` for the seeded blockfs view.");
}

fn cmd_net(args: &[&str], _hist: &[String]) {
    let devs = crate::drivers::net::snapshot();
    if devs.is_empty() {
        kprintln!("(no net devices)");
        return;
    }
    for (i, mac) in &devs {
        kprintln!(
            "  net#{:<2} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            i,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );
    }
    if matches!(args.first().copied(), Some("poll")) {
        kprintln!("polling for one frame...");
        crate::drivers::net::poll_echo();
    }
}

fn cmd_gpu(_args: &[&str], _hist: &[String]) {
    #[cfg(target_arch = "aarch64")]
    {
        let dev = match crate::drivers::virtio::gpu::primary() {
            Some(d) => d,
            None => {
                kprintln!("no virtio-gpu device registered");
                return;
            }
        };
        let (w, h) = dev.dimensions();
        dev.with_pixels(|pixels, w, h| {
            // Vertical colour bars: red, green, blue, white.
            let bands = [
                [0u8, 0, 0xff, 0],
                [0, 0xff, 0, 0],
                [0xff, 0, 0, 0],
                [0xff, 0xff, 0xff, 0],
            ];
            let bw = w as usize / bands.len();
            for y in 0..h as usize {
                for (bi, band) in bands.iter().enumerate() {
                    let x0 = bi * bw;
                    let x1 = if bi + 1 == bands.len() {
                        w as usize
                    } else {
                        x0 + bw
                    };
                    for x in x0..x1 {
                        let i = (y * w as usize + x) * 4;
                        pixels[i] = band[0];
                        pixels[i + 1] = band[1];
                        pixels[i + 2] = band[2];
                        pixels[i + 3] = band[3];
                    }
                }
            }
        });
        dev.flush();
        kprintln!("virtio-gpu: drew {}x{} colour bars and flushed", w, h);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        kprintln!("gpu: virtio-gpu only wired on aarch64 currently");
    }
}

#[cfg(target_arch = "aarch64")]
fn cmd_user(_args: &[&str], _hist: &[String]) {
    let tid = crate::proc::user::spawn_demo();
    kprintln!("spawned EL0 user demo as tid {}", tid);
}
