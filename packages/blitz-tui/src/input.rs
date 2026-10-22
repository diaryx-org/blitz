//! The terminal's keys and mouse as Blitz's events.
//!
//! A key is what the terminal reports, as it reports it: ⌃ is
//! [`Modifiers::CONTROL`] on every platform, since a terminal never sees ⌘.
//! A host whose shortcuts are ⌘ on macOS maps ⌃ over itself.

use std::str::FromStr;

use blitz_dom::BaseDocument;
use blitz_traits::events::{
    BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, KeyState,
    MouseEventButton, MouseEventButtons, PointerCoords, UiEvent,
};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use smol_str::SmolStr;

use crate::paint::Metrics;

/// How far one notch of the wheel scrolls, in rows.
pub const WHEEL_ROWS: f32 = 3.0;

/// The modifiers crossterm reports, as keyboard-types'.
pub fn modifiers(m: KeyModifiers) -> Modifiers {
    let mut mods = Modifiers::empty();
    mods.set(Modifiers::SHIFT, m.contains(KeyModifiers::SHIFT));
    mods.set(Modifiers::ALT, m.contains(KeyModifiers::ALT));
    mods.set(Modifiers::CONTROL, m.contains(KeyModifiers::CONTROL));
    mods.set(Modifiers::META, m.contains(KeyModifiers::META));
    mods
}

/// A terminal key as Blitz's key event, or `None` for a key Blitz has no
/// name for. A character typed with no ⌃ or ⌥ carries its text, which is
/// what a focused text input inserts.
pub fn key(k: &KeyEvent) -> Option<BlitzKeyEvent> {
    let mut mods = modifiers(k.modifiers);
    if k.code == KeyCode::BackTab {
        mods.insert(Modifiers::SHIFT);
    }
    let named = |n| Some((Key::Named(n), Code::Unidentified));
    let (key, code) = match k.code {
        KeyCode::Char(c) => {
            let e = char_key(c);
            Some((e.key, e.code))
        }
        KeyCode::Enter => Some((Key::Named(NamedKey::Enter), Code::Enter)),
        KeyCode::Backspace => named(NamedKey::Backspace),
        KeyCode::Delete => named(NamedKey::Delete),
        KeyCode::Insert => named(NamedKey::Insert),
        KeyCode::Tab | KeyCode::BackTab => Some((Key::Named(NamedKey::Tab), Code::Tab)),
        KeyCode::Esc => Some((Key::Named(NamedKey::Escape), Code::Escape)),
        KeyCode::Left => Some((Key::Named(NamedKey::ArrowLeft), Code::ArrowLeft)),
        KeyCode::Right => Some((Key::Named(NamedKey::ArrowRight), Code::ArrowRight)),
        KeyCode::Up => Some((Key::Named(NamedKey::ArrowUp), Code::ArrowUp)),
        KeyCode::Down => Some((Key::Named(NamedKey::ArrowDown), Code::ArrowDown)),
        KeyCode::Home => Some((Key::Named(NamedKey::Home), Code::Home)),
        KeyCode::End => Some((Key::Named(NamedKey::End), Code::End)),
        KeyCode::PageUp => Some((Key::Named(NamedKey::PageUp), Code::PageUp)),
        KeyCode::PageDown => Some((Key::Named(NamedKey::PageDown), Code::PageDown)),
        KeyCode::F(n) => Key::from_str(&format!("F{n}")).ok().map(|k| {
            (
                k,
                Code::from_str(&format!("F{n}")).unwrap_or(Code::Unidentified),
            )
        }),
        _ => None,
    }?;
    let text = match &key {
        Key::Character(s) if !mods.intersects(Modifiers::CONTROL | Modifiers::ALT) => {
            Some(SmolStr::new(s))
        }
        Key::Named(NamedKey::Enter) => Some(SmolStr::new("\r")),
        _ => None,
    };
    Some(BlitzKeyEvent {
        key,
        code,
        modifiers: mods,
        location: Location::Standard,
        is_auto_repeating: k.kind == KeyEventKind::Repeat,
        is_composing: false,
        state: if k.kind == KeyEventKind::Release {
            KeyState::Released
        } else {
            KeyState::Pressed
        },
        text,
    })
}

/// A typed character — one of a paste, say — with the physical key a US
/// layout gives it.
pub fn char_key(c: char) -> BlitzKeyEvent {
    let (key, code) = if c == '\n' || c == '\r' {
        (Key::Named(NamedKey::Enter), Code::Enter)
    } else {
        let code = if c.is_ascii_alphabetic() {
            Code::from_str(&format!("Key{}", c.to_ascii_uppercase())).unwrap_or(Code::Unidentified)
        } else if c.is_ascii_digit() {
            Code::from_str(&format!("Digit{c}")).unwrap_or(Code::Unidentified)
        } else if c == ' ' {
            Code::Space
        } else {
            Code::Unidentified
        };
        (Key::Character(c.to_string()), code)
    };
    let text = match &key {
        Key::Character(s) => SmolStr::new(s),
        _ => SmolStr::new("\r"),
    };
    BlitzKeyEvent {
        key,
        code,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: Some(text),
    }
}

/// The point a mouse event in a cell stands for, in CSS pixels: in the
/// cell's left part, so a click on a character puts a caret before it, as a
/// terminal's block cursor reads.
///
/// `x_shift` moves it sideways. The painter writes a line of text from the
/// cell its left edge rounds to, so text that starts part-way into a cell
/// is drawn up to half a cell from where it was laid out; a host whose text
/// starts `pad` pixels in passes [`Metrics::shift`]`(pad)` and the click
/// lands on the character drawn under it.
pub fn point(me: &MouseEvent, m: Metrics, x_shift: f32) -> (f32, f32) {
    (
        (me.column as f32 + 0.3) * m.cw + x_shift,
        (me.row as f32 + 0.5) * m.ch,
    )
}

impl Metrics {
    /// The [`point`] shift for text laid out `pad` pixels from a cell edge.
    pub fn shift(self, pad: f32) -> f32 {
        pad - (pad / self.cw).round() * self.cw
    }
}

/// A press, a drag, a release, a move or a turn of the wheel, as Blitz's
/// event at [`point`]. A move with no button down is a hover.
pub fn mouse(doc: &BaseDocument, me: &MouseEvent, m: Metrics, x_shift: f32) -> Option<UiEvent> {
    let (x, y) = point(me, m, x_shift);
    let scroll = doc.viewport_scroll();
    let coords = PointerCoords {
        page_x: x + scroll.x as f32,
        page_y: y + scroll.y as f32,
        screen_x: x,
        screen_y: y,
        client_x: x,
        client_y: y,
    };
    let mods = modifiers(me.modifiers);
    let wheel = |dx: f32, dy: f32| {
        Some(UiEvent::Wheel(BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(dx as f64, dy as f64),
            coords,
            buttons: MouseEventButtons::None,
            mods,
            element: Default::default(),
        }))
    };
    // winit's sign: a positive delta moves the page towards its top.
    let notch = WHEEL_ROWS * m.ch;
    match me.kind {
        MouseEventKind::ScrollUp => return wheel(0.0, notch),
        MouseEventKind::ScrollDown => return wheel(0.0, -notch),
        MouseEventKind::ScrollLeft => return wheel(notch, 0.0),
        MouseEventKind::ScrollRight => return wheel(-notch, 0.0),
        _ => {}
    }
    let button = match me.kind {
        MouseEventKind::Down(b) | MouseEventKind::Up(b) | MouseEventKind::Drag(b) => b,
        _ => MouseButton::Left,
    };
    let (button, held) = match button {
        MouseButton::Left => (MouseEventButton::Main, MouseEventButtons::Primary),
        MouseButton::Right => (MouseEventButton::Secondary, MouseEventButtons::Secondary),
        MouseButton::Middle => (MouseEventButton::Auxiliary, MouseEventButtons::Auxiliary),
    };
    let buttons = match me.kind {
        MouseEventKind::Down(_) | MouseEventKind::Drag(_) => held,
        _ => MouseEventButtons::None,
    };
    let event = BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords,
        button,
        buttons,
        mods,
        details: Default::default(),
        element: Default::default(),
        active_pointers: Default::default(),
    };
    Some(match me.kind {
        MouseEventKind::Down(_) => UiEvent::PointerDown(event),
        MouseEventKind::Up(_) => UiEvent::PointerUp(event),
        _ => UiEvent::PointerMove(event),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_letter_carries_its_text_and_a_chord_does_not() {
        let e = key(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)).unwrap();
        assert_eq!(e.key, Key::Character("a".into()));
        assert_eq!(e.code, Code::KeyA);
        assert_eq!(e.text.as_deref(), Some("a"));
        let e = key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        assert!(e.modifiers.contains(Modifiers::CONTROL));
        assert_eq!(e.text, None);
    }

    #[test]
    fn back_tab_is_shift_tab() {
        let e = key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)).unwrap();
        assert_eq!(e.key, Key::Named(NamedKey::Tab));
        assert!(e.modifiers.contains(Modifiers::SHIFT));
    }

    #[test]
    fn a_shift_is_what_moves_a_click_onto_the_drawn_text() {
        let m = Metrics { cw: 10.0, ch: 20.0 };
        // Laid out 16px in, drawn from the second cell (20px): 4px right.
        assert_eq!(m.shift(16.0), -4.0);
        assert_eq!(m.shift(20.0), 0.0);
    }
}
