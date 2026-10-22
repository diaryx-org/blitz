//! HTML pages, laid out by Blitz and painted into cells.

use blitz_dom::{Document, UserAgentStylesheet};
use blitz_html::HtmlDocument;
use blitz_tui::{FACE, Grid, LEADING, Metrics, PaintOptions, SIZE, cell_stylesheet};
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

fn options() -> PaintOptions {
    PaintOptions::new(Metrics::measure(FACE, SIZE, LEADING))
}

fn page(html: &str, opts: &PaintOptions, cols: u16, rows: u16) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(html, Default::default());
    doc.inner_mut()
        .add_user_agent_stylesheet(UserAgentStylesheet::parse(&cell_stylesheet(
            FACE, SIZE, LEADING,
        )));
    doc.inner_mut()
        .set_viewport(blitz_tui::viewport(opts.metrics, cols, rows, true));
    blitz_tui::settle(&mut doc, 0.0);
    doc
}

fn frame(html: &str, opts: &PaintOptions) -> Grid {
    let doc = page(html, opts, 40, 10);
    blitz_tui::paint(&doc.inner(), opts, 40, 10)
}

#[test]
fn a_heading_and_a_paragraph_read_as_their_text() {
    let g = frame(
        "<body style='margin:0'><h1 style='margin:0'>Title</h1><p style='margin:0'>Some body text.</p></body>",
        &options(),
    );
    let t = g.text();
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "Title", "{t}");
    assert_eq!(lines[1], "Some body text.", "{t}");
    assert!(g.get(0, 0).bold, "a heading is bold");
}

#[test]
fn a_bordered_box_is_drawn_in_box_characters() {
    let g = frame(
        "<body style='margin:0'><div style='border:1px solid white; width:100px; height:72px'></div></body>",
        &options(),
    );
    let t = g.text();
    let first = t.lines().next().unwrap();
    assert!(first.starts_with('┌') && first.ends_with('┐'), "{t}");
    assert!(t.lines().any(|l| l.starts_with('└')), "{t}");
}

#[test]
fn a_named_caret_box_is_the_terminal_cursor() {
    let mut opts = options();
    opts.cursor_classes = vec!["caret".into()];
    let g = frame(
        "<body style='margin:0'><p style='margin:0'>ab<span class='caret' style='display:inline-block;width:2px;height:1em;background:red'></span>cd</p></body>",
        &opts,
    );
    assert_eq!(g.cursor, Some((2, 0)), "{}", g.text());
    assert_eq!(g.text().lines().next(), Some("ab▏cd"));
}

#[test]
fn the_wheel_scrolls_the_page_down() {
    let opts = options();
    let body: String = (0..60)
        .map(|i| format!("<p style='margin:0'>line {i}</p>"))
        .collect();
    let mut doc = page(
        &format!("<body style='margin:0'>{body}</body>"),
        &opts,
        40,
        10,
    );
    let m = opts.metrics;
    let at = |kind| MouseEvent {
        kind,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    };
    for kind in [MouseEventKind::Moved, MouseEventKind::ScrollDown] {
        let ev = blitz_tui::input::mouse(&doc.inner(), &at(kind), m, 0.0).unwrap();
        doc.handle_ui_event(ev);
    }
    blitz_tui::settle(&mut doc, 0.0);
    let g = blitz_tui::paint(&doc.inner(), &opts, 40, 10);
    let first = g.text().lines().next().unwrap().to_string();
    assert_eq!(first, "line 3", "{}", g.text());
}
