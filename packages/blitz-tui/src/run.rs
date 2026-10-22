//! A loop that runs any Blitz [`Document`] in the terminal: an HTML page, a
//! Dioxus app, anything that implements the trait.
//!
//! Each turn the terminal's events go to the document as Blitz's own — keys
//! to the focused element, the mouse to the element under it — and the
//! document is polled, resolved and, if anything changed, painted. A host
//! that routes keys its own way (an editor that takes them before the DOM
//! does) writes its own loop from the same parts: [`crate::input`],
//! [`crate::paint()`] and [`crate::Terminal`].

use std::io;
use std::task::{Context, Waker};
use std::time::{Duration, Instant};

use blitz_dom::{Document, UserAgentStylesheet};
use blitz_traits::events::{KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use crossterm::terminal;

use crate::input;
use crate::paint::{Metrics, PaintOptions, paint};
use crate::term::{Terminal, ask_dark};

/// The face every cell is set in when an app does not choose one.
pub const FACE: &str = "Menlo, \"DejaVu Sans Mono\", monospace";
/// Its size, in CSS pixels.
pub const SIZE: f32 = 16.0;
/// The line box over the size: one cell high.
pub const LEADING: f32 = 1.5;

/// A stylesheet that sets every element in `face`, at `size` pixels on a
/// line `size × leading` high, over whatever the page says: the grid has one
/// size of character, and text laid out at another would not land in it.
pub fn cell_stylesheet(face: &str, size: f32, leading: f32) -> String {
    format!(
        "* {{ font-family: {face} !important; font-size: {size}px !important; \
         line-height: {lh}px !important; }}",
        lh = size * leading,
    )
}

pub struct Options {
    pub paint: PaintOptions,
    /// A user-agent stylesheet added before the first frame; `None` leaves
    /// the page's own styles alone. [`Options::default`]'s is
    /// [`cell_stylesheet`] of [`FACE`].
    pub stylesheet: Option<String>,
    /// Whether the terminal is dark; `None` asks it.
    pub dark: Option<bool>,
    /// Whether a key ends the loop: ⌃C or ⌃Q by default.
    pub quit: fn(&KeyEvent) -> bool,
    /// How long a turn waits for an event.
    pub tick: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            paint: PaintOptions::new(Metrics::measure(FACE, SIZE, LEADING)),
            stylesheet: Some(cell_stylesheet(FACE, SIZE, LEADING)),
            dark: None,
            quit: |k| {
                k.modifiers == KeyModifiers::CONTROL && matches!(k.code, KeyCode::Char('c' | 'q'))
            },
            tick: Duration::from_millis(50),
        }
    }
}

/// The viewport `cols` × `rows` cells make.
pub fn viewport(m: Metrics, cols: u16, rows: u16, dark: bool) -> Viewport {
    let (w, h) = m.px(cols, rows);
    let scheme = if dark {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    Viewport::new(w, h, 1.0, scheme)
}

/// Poll `doc` and lay it out until it holds still, at most a few times:
/// whether anything changed.
pub fn settle(doc: &mut dyn Document, animation_time: f64) -> bool {
    let mut any = false;
    for _ in 0..3 {
        let changed = doc.poll(Some(Context::from_waker(Waker::noop())));
        doc.inner_mut().resolve(animation_time);
        any |= changed;
        if !changed {
            break;
        }
    }
    any
}

/// Run `doc` in the terminal until a [`Options::quit`] key.
pub fn run(doc: &mut dyn Document, opts: Options) -> io::Result<()> {
    let dark = opts.dark.or_else(ask_dark).unwrap_or(true);
    if let Some(css) = &opts.stylesheet {
        doc.inner_mut()
            .add_user_agent_stylesheet(UserAgentStylesheet::parse(css));
    }
    let m = opts.paint.metrics;
    let (mut cols, mut rows) = terminal::size()?;
    doc.inner_mut().set_viewport(viewport(m, cols, rows, dark));
    let mut out = Terminal::enter()?;
    let start = Instant::now();
    let result = (|| {
        settle(doc, 0.0);
        let mut grid = paint(&doc.inner(), &opts.paint, cols, rows);
        out.draw(&grid, &doc.inner())?;
        loop {
            let mut acted = false;
            if event::poll(opts.tick)? {
                while {
                    match event::read()? {
                        Event::Key(k) => {
                            if (opts.quit)(&k) {
                                return Ok(());
                            }
                            if let Some(e) = input::key(&k) {
                                press(doc, e);
                            }
                        }
                        Event::Paste(text) => {
                            for c in text.chars() {
                                press(doc, input::char_key(c));
                            }
                        }
                        Event::Mouse(me) => {
                            // The wheel scrolls what is under the pointer:
                            // Blitz finds it by the last move.
                            if matches!(
                                me.kind,
                                MouseEventKind::ScrollUp
                                    | MouseEventKind::ScrollDown
                                    | MouseEventKind::ScrollLeft
                                    | MouseEventKind::ScrollRight
                            ) {
                                let moved = crossterm::event::MouseEvent {
                                    kind: MouseEventKind::Moved,
                                    ..me
                                };
                                let ev = input::mouse(&doc.inner(), &moved, m, 0.0);
                                if let Some(ev) = ev {
                                    doc.handle_ui_event(ev);
                                }
                            }
                            let ev = input::mouse(&doc.inner(), &me, m, 0.0);
                            if let Some(ev) = ev {
                                doc.handle_ui_event(ev);
                            }
                        }
                        _ => {}
                    }
                    event::poll(Duration::ZERO)?
                } {}
                acted = true;
            }
            // The size, asked each turn rather than trusted to resize
            // events, which a terminal can send while start-up queries read
            // stdin, where they are lost.
            let (c, r) = terminal::size()?;
            if (c, r) != (cols, rows) {
                (cols, rows) = (c, r);
                doc.inner_mut().set_viewport(viewport(m, cols, rows, dark));
                out.clear()?;
                acted = true;
            }
            if settle(doc, start.elapsed().as_secs_f64()) {
                acted = true;
            }
            if acted {
                let next = paint(&doc.inner(), &opts.paint, cols, rows);
                if next != grid {
                    out.draw(&next, &doc.inner())?;
                    grid = next;
                }
            }
        }
    })();
    out.leave()?;
    result
}

/// A key down and, as a terminal reports no releases, up again.
fn press(doc: &mut dyn Document, e: blitz_traits::events::BlitzKeyEvent) {
    if e.state == KeyState::Released {
        doc.handle_ui_event(UiEvent::KeyUp(e));
        return;
    }
    let mut up = e.clone();
    up.state = KeyState::Released;
    up.text = None;
    doc.handle_ui_event(UiEvent::KeyDown(e));
    doc.handle_ui_event(UiEvent::KeyUp(up));
}
