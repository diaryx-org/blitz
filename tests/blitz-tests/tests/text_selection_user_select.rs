//! A press Blitz may not select with starts no selection.
//!
//! Found in Diaryx, where leaf's editor cancels `mousedown` so that its own
//! caret and selection are the only ones on its page. A click on the app's
//! labels anchored a collapsed selection; the next press, in the page, was
//! cancelled and so never reset it; and that press's drag carried the old
//! anchor to the pointer, selecting every label between the two.

use blitz_test_harness::Harness;
use cursor_icon::CursorIcon;
use dioxus::prelude::*;

/// A label, then a page that cancels `mousedown`, as leaf's editor does.
fn label_then_cancelling_page() -> Element {
    rsx! {
        style { "body {{ margin: 0; font-size: 20px; }} div {{ height: 40px; width: max-content; }}" }
        div { id: "label", "Library name" }
        div { id: "page", onmousedown: move |e: MouseEvent| e.prevent_default(), "The page's own text" }
    }
}

/// Labels inside a container that may not be selected, two levels down, then
/// a page; and one label that says it may be, inside the same container.
const CHROME: &str = r#"<!DOCTYPE html>
<html><head><style>
body { margin: 0; font-size: 20px; }
div { height: 40px; width: max-content; }
.chrome { user-select: none; height: auto; }
.again { user-select: text; }
</style></head>
<body>
<div class="chrome"><div><span id="label">Library name</span></div><div id="own" class="again">Selectable title</div></div>
<div id="page">The page's own text</div>
</body></html>
"#;

/// Whether text is measured at all: with no usable font it is 0x0, and no
/// press can land on it.
fn has_fonts<D: blitz_dom::Document>(harness: &Harness<D>, selector: &str) -> bool {
    let (x, y) = harness.center_of(selector);
    harness.hit(x, y).is_some_and(|hit| hit.is_text)
}

fn selected<D: blitz_dom::Document>(harness: &Harness<D>) -> Option<String> {
    let doc = harness.base();
    doc.has_text_selection()
        .then(|| doc.get_selected_text())
        .flatten()
}

#[test]
fn a_cancelled_press_does_not_drag_an_earlier_anchor() {
    let mut harness = Harness::from_component(label_then_cancelling_page);
    if !has_fonts(&harness, "#label") {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    harness.click("#label");
    let (x, y) = harness.center_of("#page");
    harness.drag((x, y), (x + 10.0, y + 2.0), 2);

    assert_eq!(selected(&harness), None);
}

#[test]
fn a_drag_from_text_under_user_select_none_selects_nothing() {
    let mut harness = Harness::from_html(CHROME);
    if !has_fonts(&harness, "#label") {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    let from = harness.center_of("#label");
    let to = harness.center_of("#page");
    harness.drag(from, to, 4);

    assert_eq!(selected(&harness), None);
}

#[test]
fn a_press_on_text_under_user_select_none_clears_the_selection() {
    let mut harness = Harness::from_html(CHROME);
    if !has_fonts(&harness, "#page") {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    let (x, y) = harness.center_of("#page");
    harness.drag((x - 40.0, y), (x + 40.0, y), 4);
    assert!(selected(&harness).is_some(), "the page's text selects");

    harness.click("#label");
    assert_eq!(selected(&harness), None);
}

#[test]
fn text_under_user_select_none_shows_no_text_cursor() {
    let mut harness = Harness::from_html(CHROME);
    if !has_fonts(&harness, "#label") {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    let (x, y) = harness.center_of("#label");
    harness.move_mouse_to(x, y);
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::Default));

    let (x, y) = harness.center_of("#own");
    harness.move_mouse_to(x, y);
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::Text));
}

#[test]
fn user_select_text_inside_user_select_none_still_selects() {
    let mut harness = Harness::from_html(CHROME);
    if !has_fonts(&harness, "#own") {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    let (x, y) = harness.center_of("#own");
    harness.drag((x - 30.0, y), (x + 30.0, y), 4);

    let text = selected(&harness).expect("a selection");
    assert!(
        !text.is_empty() && "Selectable title".contains(&text),
        "{text:?}"
    );
}
