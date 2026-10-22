//! A Dioxus app in a terminal: the counter, run by blitz-tui instead of a
//! window.
//!
//!     cargo run -p tui_counter
//!
//! Click the buttons, or click the card and press + and -. ⌃C quits.

use blitz_dom::DocumentConfig;
use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use dioxus_native_dom::DioxusDocument;

fn app() -> Element {
    let mut count = use_signal(|| 0);
    rsx! {
        style { {CSS} }
        div {
            class: "card",
            tabindex: "0",
            onkeydown: move |e| match e.key() {
                Key::Character(c) if c == "+" => count += 1,
                Key::Character(c) if c == "-" => count -= 1,
                _ => {}
            },
            h1 { "Count: {count}" }
            div { class: "buttons",
                button { onclick: move |_| count += 1, "Increment" }
                button { onclick: move |_| count -= 1, "Decrement" }
                button { onclick: move |_| count.set(0), "Reset" }
            }
            p { "Click a button, or click here and press + and -." }
        }
    }
}

const CSS: &str = "
body { margin: 0; background: #1e1e2e; color: #cdd6f4; }
.card { margin: 24px 20px; padding: 24px 20px; border: 1px solid #89b4fa; }
h1 { margin: 0 0 24px 0; color: #f9e2af; }
.buttons { display: flex; gap: 20px; }
button { background: #313244; color: #cdd6f4; border: 0; padding: 0 10px; }
button:hover { background: #45475a; }
p { margin: 24px 0 0 0; color: #a6adc8; }
";

fn document() -> DioxusDocument {
    let mut doc = DioxusDocument::new(VirtualDom::new(app), DocumentConfig::default());
    doc.initial_build();
    doc
}

fn main() -> std::io::Result<()> {
    blitz_tui::run(&mut document(), blitz_tui::Options::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::{Document, UserAgentStylesheet};
    use blitz_traits::events::UiEvent;
    use blitz_tui::{FACE, LEADING, Options, SIZE, cell_stylesheet};
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    struct Page {
        doc: DioxusDocument,
        opts: Options,
    }

    impl Page {
        fn new() -> Self {
            let opts = Options::default();
            let mut doc = document();
            doc.inner_mut()
                .add_user_agent_stylesheet(UserAgentStylesheet::parse(&cell_stylesheet(
                    FACE, SIZE, LEADING,
                )));
            doc.inner_mut()
                .set_viewport(blitz_tui::viewport(opts.paint.metrics, 80, 24, true));
            let mut p = Self { doc, opts };
            p.settle();
            p
        }

        fn settle(&mut self) {
            blitz_tui::settle(&mut self.doc, 0.0);
        }

        fn text(&self) -> String {
            blitz_tui::paint(&self.doc.inner(), &self.opts.paint, 80, 24).text()
        }

        /// A click in the cell where `label` begins.
        fn click(&mut self, label: &str) {
            let t = self.text();
            let (row, line) = t
                .lines()
                .enumerate()
                .find(|(_, l)| l.contains(label))
                .unwrap_or_else(|| panic!("{label} in\n{t}"));
            let column = line[..line.find(label).unwrap()].chars().count() as u16;
            let m = self.opts.paint.metrics;
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                let me = MouseEvent {
                    kind,
                    column,
                    row: row as u16,
                    modifiers: KeyModifiers::NONE,
                };
                let ev = blitz_tui::input::mouse(&self.doc.inner(), &me, m, 0.0).unwrap();
                self.doc.handle_ui_event(ev);
            }
            self.settle();
        }

        fn press(&mut self, c: char) {
            let k = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            let ev = blitz_tui::input::key(&k).unwrap();
            self.doc.handle_ui_event(UiEvent::KeyDown(ev));
            self.settle();
        }
    }

    #[test]
    fn the_counter_reads_as_text_in_its_box() {
        let t = Page::new().text();
        assert!(t.contains("Count: 0"), "{t}");
        assert!(t.contains("Increment") && t.contains("Reset"), "{t}");
        assert!(t.contains('┌'), "{t}");
    }

    #[test]
    fn a_click_on_a_button_counts() {
        let mut p = Page::new();
        p.click("Increment");
        p.click("Increment");
        p.click("Decrement");
        let t = p.text();
        assert!(t.contains("Count: 1"), "{t}");
    }

    #[test]
    fn a_key_reaches_the_focused_element() {
        let mut p = Page::new();
        p.click("Click a button");
        p.press('+');
        p.press('+');
        let t = p.text();
        assert!(t.contains("Count: 2"), "{t}");
    }
}
