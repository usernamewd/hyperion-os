# UI API guide

The Hyperion UI stack is intentionally tiny — it's a building block, not
a finished window manager. The pieces are:

```
                ┌─────────────────────────────┐
                │ Widget tree (Panel,Label..) │
                └──────────────┬──────────────┘
                               │  render(&mut Canvas)
                               ▼
                ┌─────────────────────────────┐
                │ Canvas  (rect/line/text)    │
                └──────────────┬──────────────┘
                               │  &mut Framebuffer
                               ▼
                ┌─────────────────────────────┐
                │ Compositor (z-ordered)      │
                └──────────────┬──────────────┘
                               │  blits layers
                               ▼
                ┌─────────────────────────────┐
                │ Monitor + Framebuffer       │
                └─────────────────────────────┘
```

## Framebuffers

```rust
use hyperion_kernel::display::framebuffer::{Framebuffer, PixelFormat};

let mut fb = Framebuffer::new(640, 480, PixelFormat::Bgra8);
fb.clear([0x10, 0x10, 0x18, 0xff]);
fb.fill_rect(20, 20, 80, 40, [0x33, 0x99, 0xff, 0xff]);
```

Pixel format is `Bgra8` (default for the QEMU virtio-gpu) or `Rgba8`.
`encode_rgba()` always returns RGBA8, regardless of the internal layout.

## Monitors

A `Monitor` wraps a single framebuffer and is registered with the
display subsystem at boot:

```rust
let mon = Arc::new(Monitor::new_virtual("ext", 1024, 768, PixelFormat::Bgra8));
display::register_monitor(mon);
```

`display::get(0)` returns the default boot monitor (`1280x720` virtual);
`display::list()` enumerates all of them.

## Canvas

```rust
use hyperion_kernel::ui::Canvas;

display::get(0).with_framebuffer(|fb| {
    let mut c = Canvas::new(fb);
    c.clear([0x10, 0x10, 0x18, 0xff]);
    c.fill_rect(20, 20, 200, 60, [0x33, 0x55, 0xcc, 0xff]);
    c.stroke_rect(20, 20, 200, 60, [0xff, 0xff, 0xff, 0xff]);
    c.line(0, 0, 199, 59, [0xff, 0x44, 0x44, 0xff]);
    c.draw_text(30, 35, "hello", [0xff, 0xff, 0xff, 0xff]);
});
```

The same API is exposed as the **`CanvasOps` trait** in
[`hyperion-os-api`](../libos-api) so out-of-tree code can target it
without depending on the kernel crate.

## Font

The default font is a 95-glyph 8×8 bitmap covering printable ASCII
(`0x20..=0x7E`). Each glyph is 8 rows of 8 bits, MSB-first. The exact
glyph data lives in `kernel/src/ui/font.rs`. To use a different font,
implement your own struct with a `draw_char(c, x, y, &mut Framebuffer)`
method — the rest of the UI stack works through `Canvas::draw_text`,
which calls into the font, so swapping is trivial.

## Widgets

```rust
use hyperion_kernel::ui::{Panel, Label, Button};

let mut root = Panel::new(0, 0, 640, 480, [0x10, 0x10, 0x18, 0xff], None);
root.add(Box::new(Label::new(20, 20, "Welcome", [0xff, 0xff, 0xff, 0xff])));
root.add(Box::new(Button::new(20, 60, 120, 32, "OK",
    [0xff, 0xff, 0xff, 0xff], [0x33, 0x55, 0xcc, 0xff])));
display::get(0).with_framebuffer(|fb| {
    let mut c = Canvas::new(fb);
    root.render(&mut c);
});
```

`Panel` is a container; `Label` draws text; `Button` is a labelled
filled rectangle. All three implement the `Widget` trait, so you can
build your own widgets the same way:

```rust
pub struct ProgressBar { /* ... */ }
impl Widget for ProgressBar {
    fn render(&self, c: &mut Canvas) { /* ... */ }
}
```

## Compositor

The compositor stacks multiple framebuffers (each with `(x, y, z)`)
and renders them onto a destination monitor. Use it when you have
several independent producers (a status bar, a window, a cursor):

```rust
use hyperion_kernel::display::compositor::{Compositor, Layer};
use alloc::sync::Arc;
use spin::Mutex;

let mon = display::get(0).unwrap();
let mut comp = Compositor::new(mon.clone());

let bar = Arc::new(Mutex::new(Framebuffer::new(1280, 24, PixelFormat::Bgra8)));
bar.lock().clear([0x33, 0x33, 0x33, 0xff]);

comp.set_background([0x05, 0x05, 0x10, 0xff]);
comp.push_layer(Layer { fb: bar.clone(), x: 0, y: 0, z: 0 });
comp.render();
```

Higher `z` draws on top. The layer's alpha channel is honoured: an
`alpha == 0` pixel is fully transparent.

## Trying it live

The `demo` shell command exercises the canvas + font path on the
default monitor and is a good copy-paste starting point. See
`fn cmd_demo` in `kernel/src/shell/commands.rs`.
