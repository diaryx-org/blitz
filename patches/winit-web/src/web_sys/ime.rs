//! The input method, through a hidden `<textarea>` (leaf patch).
//!
//! A page has no API to raise an on-screen keyboard or to talk to an input
//! method: both come only to an editable element with focus. So enabling the
//! IME focuses a textarea of winit's own, laid over the IME cursor area and
//! kept invisible, and disabling it hands focus back to the canvas.
//!
//! The textarea holds the surrounding text the application sends, with its
//! selection, so the keyboard's suggestions, autocorrection and
//! capitalisation see the text the caret is in. Whatever the input method
//! then does to the textarea — a commit, a composition, a correction, a
//! deletion — is read back as a difference from what it held, and sent as
//! `Ime::DeleteSurrounding` and `Ime::Commit`. A composition is committed as
//! it goes rather than shown as preedit: the browser does not say reliably
//! which existing text a composition took over (an Android keyboard composes
//! the word the caret lands in), and the difference does.
//!
//! Keys that edit or move (Backspace, Enter, arrows, shortcuts) still go as
//! `KeyboardInput`, from the textarea as from the canvas, and the textarea
//! does not act on them. A soft keyboard that sends no key for them (key
//! code 229, as Android's do) has its `beforeinput` turned into the key.
//!
//! Focus: on a touch device a keyboard comes up only for a focus given
//! inside a tap, and a tap on a button must not bring it up. So there the
//! IME is enabled only by a request made while a tap is being handled — an
//! editor's, for a tap on its text, or a field's, as it takes focus — and a
//! request made outside one (an editor that asks as it mounts) is refused,
//! leaving the IME disabled so the next tap on the text asks again. The
//! keyboard closed by the person (its Done, a swipe) blurs the textarea,
//! which disables the IME the same way. Where there is a real keyboard the
//! IME is enabled whenever it is asked for, and a click on the canvas gives
//! the textarea its focus back.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use dpi::{LogicalPosition, LogicalSize};
use smol_str::SmolStr;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CompositionEvent, Document, Element, EventTarget, FocusEvent, HtmlCanvasElement,
    HtmlTextAreaElement, InputEvent, KeyboardEvent, PointerEvent,
};
use winit_core::event::{Ime, WindowEvent};
use winit_core::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey};
use winit_core::window::{ImeCapabilities, ImeRequest, ImeRequestData, ImeRequestError};

use super::event;
use super::event_handle::EventListenerHandle;

/// The attribute that marks the textarea, so the canvas's focus handling
/// can tell focus moving to it from focus leaving the page.
const MARK: &str = "data-winit-ime";

pub type KeyHandler =
    Rc<RefCell<dyn FnMut(PhysicalKey, Key, Option<SmolStr>, KeyLocation, bool, ModifiersState)>>;

pub type Sink = Rc<dyn Fn(Vec<WindowEvent>)>;

/// Whether `target` is the IME's textarea.
pub fn is_area(target: Option<EventTarget>) -> bool {
    target.and_then(|t| t.dyn_into::<Element>().ok()).is_some_and(|e| e.has_attribute(MARK))
}

/// Whether the IME's textarea has focus, which a press on the canvas must
/// not take from it.
pub fn area_focused(document: &Document) -> bool {
    document.active_element().is_some_and(|e| e.has_attribute(MARK))
}

/// The textarea's text and selection, in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Text {
    value: String,
    start: usize,
    end: usize,
}

impl Text {
    fn read(area: &HtmlTextAreaElement) -> Self {
        let value = area.value();
        let start = byte_of(&value, area.selection_start().ok().flatten().unwrap_or(0));
        let end = byte_of(&value, area.selection_end().ok().flatten().unwrap_or(0));
        Text { start: start.min(end), end: start.max(end), value }
    }
}

/// The byte where UTF-16 unit `at` falls, clamped to the text.
fn byte_of(text: &str, at: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in text.char_indices() {
        if units >= at {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

fn units_of(text: &str, byte: usize) -> u32 {
    text[..byte.min(text.len())].encode_utf16().count() as u32
}

/// What turns `old` into `new`, as the IME events an editor takes: the
/// text removed round the old selection, and the text put in its place.
fn plan(old: &Text, new: &Text) -> Vec<Ime> {
    let (o, n) = (old.value.as_str(), new.value.as_str());
    if o == n {
        return Vec::new();
    }
    let prefix = o
        .char_indices()
        .zip(n.chars())
        .find(|((_, a), b)| a != b)
        .map(|((i, _), _)| i)
        .unwrap_or_else(|| o.len().min(n.len()));
    // A common prefix stops at the selection: what was typed at the caret
    // goes after what is before it, even where the two read the same.
    let p = prefix.min(old.start);
    let room = (o.len() - p).min(n.len() - p).min(o.len() - old.end);
    let suffix: usize = o[p..]
        .chars()
        .rev()
        .zip(n[p..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let s = suffix.min(room);
    let (gone, put) = ((p, o.len() - s), &n[p..n.len() - s]);
    let mut events = Vec::new();
    if gone == (old.start, old.end) {
        // In place of the selection, or at the caret.
    } else if gone.0 <= old.start && old.end <= gone.1 {
        if old.start != old.end {
            events.push(Ime::Commit(String::new()));
        }
        events.push(Ime::DeleteSurrounding {
            before_bytes: old.start - gone.0,
            after_bytes: gone.1 - old.end,
        });
    } else {
        // Removed apart from the selection: not something a keyboard does.
        return Vec::new();
    }
    if !put.is_empty() {
        events.push(Ime::Preedit(String::new(), None));
        events.push(Ime::Commit(put.to_owned()));
    }
    events
}

/// A listener in the capture or the bubble phase, removed when dropped.
struct Listener {
    target: EventTarget,
    name: &'static str,
    capture: bool,
    closure: Closure<dyn FnMut(web_sys::Event)>,
}

impl Listener {
    fn new(
        target: EventTarget,
        name: &'static str,
        capture: bool,
        closure: Closure<dyn FnMut(web_sys::Event)>,
    ) -> Self {
        let _ = target.add_event_listener_with_callback_and_bool(
            name,
            closure.as_ref().unchecked_ref(),
            capture,
        );
        Listener { target, name, capture, closure }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback_and_bool(
            self.name,
            self.closure.as_ref().unchecked_ref(),
            self.capture,
        );
    }
}

pub struct ImeHandler(Rc<Inner>);

struct Inner {
    window: web_sys::Window,
    document: Document,
    canvas: HtmlCanvasElement,
    has_focus: Rc<Cell<bool>>,
    touch: bool,
    /// Whether a tap is being handled: from the capture of a `pointerup` at
    /// the window to its bubbling there, round the canvas's own listeners.
    tapping: Rc<Cell<bool>>,
    _tap_listeners: Vec<Box<dyn Any>>,
    area: RefCell<Option<HtmlTextAreaElement>>,
    listeners: RefCell<Vec<Box<dyn Any>>>,
    capabilities: Cell<Option<ImeCapabilities>>,
    /// What the textarea held when last read or set.
    known: RefCell<Text>,
    /// Surrounding text that came while a composition ran, set when it ends.
    pending: RefCell<Option<Text>>,
    composing: Cell<bool>,
    sink: RefCell<Option<Sink>>,
    press: RefCell<Option<KeyHandler>>,
    release: RefCell<Option<KeyHandler>>,
}

impl ImeHandler {
    pub fn new(
        window: web_sys::Window,
        document: Document,
        canvas: HtmlCanvasElement,
        has_focus: Rc<Cell<bool>>,
    ) -> Self {
        let touch = window
            .match_media("(pointer: coarse)")
            .ok()
            .flatten()
            .is_some_and(|m| m.matches());
        let tapping = Rc::new(Cell::new(false));
        let mut tap_listeners: Vec<Box<dyn Any>> = Vec::new();
        for (name, capture, value) in
            [("pointerup", true, true), ("pointerup", false, false), ("pointerdown", true, false)]
        {
            let tapping = Rc::clone(&tapping);
            tap_listeners.push(Box::new(Listener::new(
                window.clone().into(),
                name,
                capture,
                Closure::<dyn FnMut(web_sys::Event)>::new(move |_| tapping.set(value)),
            )));
        }
        ImeHandler(Rc::new(Inner {
            window,
            document,
            canvas,
            has_focus,
            touch,
            tapping,
            _tap_listeners: tap_listeners,
            area: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            capabilities: Cell::new(None),
            known: RefCell::new(Text::default()),
            pending: RefCell::new(None),
            composing: Cell::new(false),
            sink: RefCell::new(None),
            press: RefCell::new(None),
            release: RefCell::new(None),
        }))
    }

    pub fn set_sink(&self, sink: Sink) {
        *self.0.sink.borrow_mut() = Some(sink);
    }

    pub fn set_press(&self, handler: KeyHandler) {
        *self.0.press.borrow_mut() = Some(handler);
    }

    pub fn set_release(&self, handler: KeyHandler) {
        *self.0.release.borrow_mut() = Some(handler);
    }

    pub fn capabilities(&self) -> Option<ImeCapabilities> {
        self.0.capabilities.get()
    }

    pub fn request(&self, request: ImeRequest) -> Result<(), ImeRequestError> {
        let inner = &self.0;
        match request {
            ImeRequest::Enable(enable) => {
                if inner.capabilities.get().is_some() {
                    return Err(ImeRequestError::AlreadyEnabled);
                }
                if inner.touch && !inner.tapping.get() {
                    return Err(ImeRequestError::NotSupported);
                }
                let (capabilities, data) = enable.into_raw();
                let area = Inner::area(inner);
                inner.capabilities.set(Some(capabilities));
                inner.composing.set(false);
                *inner.pending.borrow_mut() = None;
                area.set_value("");
                *inner.known.borrow_mut() = Text::default();
                inner.apply(data);
                inner.emit(vec![WindowEvent::Ime(Ime::Enabled)]);
                inner.focus_area();
                Ok(())
            },
            ImeRequest::Update(data) => {
                if inner.capabilities.get().is_none() {
                    return Err(ImeRequestError::NotEnabled);
                }
                inner.apply(data);
                Ok(())
            },
            ImeRequest::Disable => {
                if inner.capabilities.take().is_some() {
                    inner.composing.set(false);
                    *inner.pending.borrow_mut() = None;
                    if area_focused(&inner.document) {
                        let _ = inner.canvas.focus();
                    }
                    inner.emit(vec![WindowEvent::Ime(Ime::Disabled)]);
                }
                Ok(())
            },
            _ => Err(ImeRequestError::NotSupported),
        }
    }
}

impl Inner {
    fn emit(&self, events: Vec<WindowEvent>) {
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            if !events.is_empty() {
                sink(events);
            }
        }
    }

    fn focus_area(&self) {
        let area = self.area.borrow().clone();
        if let Some(area) = area {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = area.focus_with_options(&options);
        }
    }

    fn apply(&self, data: ImeRequestData) {
        let Some(area) = self.area.borrow().clone() else { return };
        if let Some((position, size)) = data.cursor_area {
            let scale = super::scale_factor(&self.window);
            let at: LogicalPosition<f64> = position.to_logical(scale);
            let size: LogicalSize<f64> = size.to_logical(scale);
            let bounds = self.canvas.get_bounding_client_rect();
            let style = area.style();
            let _ = style.set_property("left", &format!("{}px", bounds.x() + at.x));
            let _ = style.set_property("top", &format!("{}px", bounds.y() + at.y));
            let _ = style.set_property("height", &format!("{}px", size.height.max(1.0)));
        }
        if let Some(around) = data.surrounding_text {
            let (cursor, anchor) = (around.cursor(), around.anchor());
            let text = Text {
                start: cursor.min(anchor),
                end: cursor.max(anchor),
                value: around.into_text(),
            };
            if self.composing.get() {
                *self.pending.borrow_mut() = Some(text);
            } else {
                self.set(&area, text);
            }
        }
    }

    fn set(&self, area: &HtmlTextAreaElement, text: Text) {
        if Text::read(area) != text {
            if area.value() != text.value {
                area.set_value(&text.value);
            }
            let _ = area.set_selection_range(
                units_of(&text.value, text.start),
                units_of(&text.value, text.end),
            );
        }
        *self.known.borrow_mut() = text;
    }

    /// What the input method did to the textarea, sent as IME events.
    fn read_back(&self) {
        let Some(area) = self.area.borrow().clone() else { return };
        if self.capabilities.get().is_none() {
            return;
        }
        let new = Text::read(&area);
        let old = std::mem::replace(&mut *self.known.borrow_mut(), new.clone());
        let events = plan(&old, &new);
        self.emit(events.into_iter().map(WindowEvent::Ime).collect());
    }

    fn key(&self, press: bool, code: KeyCode, named: NamedKey, text: Option<&str>) {
        let handler = if press { self.press.borrow().clone() } else { self.release.borrow().clone() };
        if let Some(handler) = handler {
            (handler.borrow_mut())(
                PhysicalKey::Code(code),
                Key::Named(named),
                text.map(SmolStr::new),
                KeyLocation::Standard,
                false,
                ModifiersState::empty(),
            );
        }
    }

    /// The textarea, made and listened to the first time it is needed.
    fn area(this: &Rc<Self>) -> HtmlTextAreaElement {
        if let Some(area) = this.area.borrow().clone() {
            return area;
        }
        let area: HtmlTextAreaElement = this
            .document
            .create_element("textarea")
            .expect("a textarea")
            .unchecked_into();
        for (name, value) in [
            (MARK, ""),
            ("autocomplete", "off"),
            ("autocapitalize", "sentences"),
            ("tabindex", "-1"),
            ("aria-hidden", "true"),
            (
                "style",
                // 16px, or iOS zooms the page to it on focus.
                "position:fixed;left:0;top:0;width:2px;height:1em;margin:0;padding:0;border:0;\
                 outline:none;resize:none;overflow:hidden;white-space:pre;font-size:16px;\
                 line-height:1;opacity:0;color:transparent;background:transparent;\
                 caret-color:transparent;pointer-events:none;",
            ),
        ] {
            let _ = area.set_attribute(name, value);
        }
        let parent = this.canvas.parent_node().or_else(|| this.document.body().map(Into::into));
        if let Some(parent) = parent {
            let _ = parent.append_child(&area);
        }
        *this.area.borrow_mut() = Some(area.clone());

        let weak = Rc::downgrade(this);
        let mut listeners: Vec<Box<dyn Any>> = Vec::new();
        let target: EventTarget = area.clone().into();

        fn on<E, F>(target: &EventTarget, name: &'static str, weak: &Weak<Inner>, mut f: F) -> Box<dyn Any>
        where
            E: 'static + AsRef<web_sys::Event> + wasm_bindgen::convert::FromWasmAbi,
            F: 'static + FnMut(&Inner, E),
        {
            let weak = weak.clone();
            Box::new(EventListenerHandle::new(
                target.clone(),
                name,
                Closure::<dyn FnMut(E)>::new(move |e: E| {
                    if let Some(inner) = weak.upgrade() {
                        f(&inner, e)
                    }
                }),
            ))
        }

        listeners.push(on(&target, "keydown", &weak, |inner, e: KeyboardEvent| {
            inner.on_key(true, &e)
        }));
        listeners.push(on(&target, "keyup", &weak, |inner, e: KeyboardEvent| {
            inner.on_key(false, &e)
        }));
        listeners.push(on(&target, "beforeinput", &weak, |inner, e: InputEvent| {
            if e.is_composing() {
                return;
            }
            let key = match e.input_type().as_str() {
                "insertLineBreak" | "insertParagraph" => {
                    Some((KeyCode::Enter, NamedKey::Enter, Some("\r")))
                },
                "deleteContentBackward" => Some((KeyCode::Backspace, NamedKey::Backspace, None)),
                "deleteContentForward" => Some((KeyCode::Delete, NamedKey::Delete, None)),
                _ => None,
            };
            if let Some((code, named, text)) = key {
                e.prevent_default();
                inner.key(true, code, named, text);
                inner.key(false, code, named, text);
            }
        }));
        listeners.push(on(&target, "input", &weak, |inner, _: InputEvent| inner.read_back()));
        listeners.push(on(&target, "compositionstart", &weak, |inner, _: CompositionEvent| {
            inner.composing.set(true)
        }));
        listeners.push(on(&target, "compositionend", &weak, |inner, _: CompositionEvent| {
            inner.composing.set(false);
            inner.read_back();
            let pending = inner.pending.borrow_mut().take();
            let area = inner.area.borrow().clone();
            if let (Some(text), Some(area)) = (pending, area) {
                inner.set(&area, text);
            }
        }));
        listeners.push(on(&target, "focus", &weak, |inner, _: FocusEvent| {
            if !inner.has_focus.replace(true) {
                inner.emit(vec![WindowEvent::Focused(true)]);
            }
        }));
        listeners.push(on(&target, "blur", &weak, |inner, e: FocusEvent| {
            let to = e.related_target();
            let to_canvas = to
                .and_then(|t| t.dyn_into::<HtmlCanvasElement>().ok())
                .is_some_and(|c| c == inner.canvas);
            if !to_canvas && inner.has_focus.replace(false) {
                inner.emit(vec![WindowEvent::Focused(false)]);
            }
            // The keyboard closed by the person: the IME with it, so that a
            // tap on the text asks for it again.
            if inner.touch && inner.capabilities.take().is_some() {
                inner.composing.set(false);
                *inner.pending.borrow_mut() = None;
                inner.emit(vec![WindowEvent::Ime(Ime::Disabled)]);
            }
        }));
        // A click on the canvas that took focus from the textarea while the
        // IME is enabled gives it back. Not under a finger: there, a tap
        // that is not on text must leave the keyboard down.
        if !this.touch {
            let canvas: EventTarget = this.canvas.clone().into();
            listeners.push(on(&canvas, "pointerup", &weak, |inner, _: PointerEvent| {
                if inner.capabilities.get().is_some() && !area_focused(&inner.document) {
                    inner.focus_area();
                }
            }));
        }
        *this.listeners.borrow_mut() = listeners;
        area
    }

    fn on_key(&self, press: bool, e: &KeyboardEvent) {
        // The input method's own: a composition, or a soft keyboard that
        // says nothing of the key.
        if e.is_composing() || e.key_code() == 229 {
            return;
        }
        let shortcut = e.ctrl_key() || e.meta_key();
        match event::key(e) {
            Key::Character(_) if !shortcut => return,
            Key::Dead(_) | Key::Unidentified(_) => return,
            _ => {},
        }
        e.prevent_default();
        let handler = if press { self.press.borrow().clone() } else { self.release.borrow().clone() };
        if let Some(handler) = handler {
            (handler.borrow_mut())(
                event::key_code(e),
                event::key(e),
                event::key_text(e),
                event::key_location(e),
                e.repeat(),
                event::keyboard_modifiers(e),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(value: &str, start: usize, end: usize) -> Text {
        Text { value: value.into(), start, end }
    }

    #[test]
    fn typing_at_the_caret_commits() {
        assert_eq!(plan(&t("a", 1, 1), &t("aa", 2, 2)), vec![
            Ime::Preedit(String::new(), None),
            Ime::Commit("a".into())
        ]);
    }

    #[test]
    fn a_correction_deletes_then_commits() {
        assert_eq!(plan(&t("teh ", 4, 4), &t("the ", 4, 4)), vec![
            Ime::DeleteSurrounding { before_bytes: 3, after_bytes: 0 },
            Ime::Preedit(String::new(), None),
            Ime::Commit("he ".into()),
        ]);
    }

    #[test]
    fn a_deletion_before_the_caret() {
        assert_eq!(plan(&t("ab", 2, 2), &t("a", 1, 1)), vec![Ime::DeleteSurrounding {
            before_bytes: 1,
            after_bytes: 0
        }]);
    }

    #[test]
    fn typing_over_a_selection() {
        assert_eq!(plan(&t("abc", 1, 2), &t("axc", 2, 2)), vec![
            Ime::Preedit(String::new(), None),
            Ime::Commit("x".into())
        ]);
    }
}
