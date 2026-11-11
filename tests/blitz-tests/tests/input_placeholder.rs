//! An empty text input shows its `placeholder`, faded, and stops showing it
//! once it holds text of its own.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: usize = 300;
const H: usize = 60;

/// The pixels of a white page holding one input with `attrs`, black text
/// at 24px, no border.
fn render(attrs: &str) -> Vec<[u8; 3]> {
    let html = format!(
        r#"<html><body style="margin:0; background:#ffffff;">
            <input {attrs} style="display:block; width:280px; height:40px; margin:10px; padding:0 4px;
                border:none; outline:none; background:#ffffff; color:#000000; font-size:24px;">
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(W as u32, H as u32, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, W as u32, H as u32, 0, 0),
        W as u32,
        H as u32,
    );
    buffer.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
}

/// How many pixels are not the page's white, and the darkest channel among
/// them.
fn ink(pixels: &[[u8; 3]]) -> (usize, u8) {
    let marked: Vec<_> = pixels.iter().filter(|p| **p != [255, 255, 255]).collect();
    let darkest = marked
        .iter()
        .flat_map(|p| p.iter())
        .copied()
        .min()
        .unwrap_or(255);
    (marked.len(), darkest)
}

#[test]
fn an_empty_input_shows_its_placeholder_faded() {
    let (none, _) = ink(&render(r#"type="text""#));
    assert_eq!(none, 0, "an empty input with no placeholder draws nothing");
    let (marked, darkest) = ink(&render(r#"type="text" placeholder="Search Journal""#));
    assert!(marked > 50, "the placeholder is drawn: {marked} pixels");
    // Black at 0.54 over white: grey, never the text's own black.
    assert!(darkest > 90, "faded: its darkest channel is {darkest}");
}

#[test]
fn an_input_with_text_shows_no_placeholder() {
    let with = render(r#"type="text" value="Harbour" placeholder="Search Journal""#);
    let without = render(r#"type="text" value="Harbour""#);
    assert!(with == without, "its text alone, whatever its placeholder");
    let (_, darkest) = ink(&with);
    assert!(darkest < 60, "the text itself at full strength: {darkest}");
}
