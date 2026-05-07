//! UI building API.
//!
//! These primitives are intended to be used by both:
//!
//! * **In-kernel UI demos** (e.g. a boot splash drawn directly to a
//!   monitor).
//! * **Userspace toolkits** that you write on top of Hyperion to ship a
//!   custom OS — they call out via the [`crate::api`] re-exports of these
//!   types and never reach into kernel internals.
//!
//! The model is dead-simple: a [`Canvas`] is a drawable region with
//! pixel/line/rect/text helpers. A [`widgets::Widget`] is anything that
//! knows how to render itself onto a Canvas.

pub mod canvas;
pub mod font;
pub mod widgets;

pub use canvas::Canvas;
pub use font::Font8x8;
pub use widgets::{Button, Label, Panel, Widget};
