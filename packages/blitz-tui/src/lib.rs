//! Paint a Blitz [`Document`](blitz_dom::Document) into a terminal's
//! character cells.
//!
//! Blitz builds the DOM, resolves the styles and lays the page out exactly
//! as it does for a window; this crate replaces only the last step, the one
//! blitz-paint does for pixels. [`paint()`] walks the laid-out tree and
//! writes a [`Grid`] of cells, [`Terminal`] writes a grid out (pictures
//! through the terminal's graphics protocol), and [`input`] turns the
//! terminal's keys and mouse into Blitz's events. [`run()`] puts them
//! together for any document:
//!
//! ```no_run
//! let mut doc = blitz_html::HtmlDocument::from_html("<h1>Hello</h1>", Default::default());
//! blitz_tui::run(&mut doc, blitz_tui::Options::default()).unwrap();
//! ```
//!
//! A cell is one advance of a monospace face wide and one line high
//! ([`Metrics`]). Text set in that face at that size lands one character to
//! a cell, so the page's own layout — wrapping, hit-testing, a caret — counts
//! in cells without knowing it; [`Options::default`] sets every element in
//! it. Text at other sizes is drawn a character to a cell along its line,
//! and boxes round to the cells their centres cover.

pub mod input;
pub mod paint;
pub mod pictures;
pub mod run;
pub mod term;
#[cfg(feature = "winit")]
pub mod window;

pub use paint::{Cell, Grid, Metrics, PaintOptions, Picture, Rgb, paint};
pub use pictures::{Decoder, Pictures};
pub use run::{FACE, LEADING, Options, SIZE, cell_stylesheet, run, settle, viewport};
pub use term::{Terminal, ask_dark};
#[cfg(feature = "winit")]
pub use window::TermWindow;
