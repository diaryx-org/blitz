// Copyright 2026 diaryx-org. Licensed under the Apache License, Version 2.0
// (found in the LICENSE-APACHE file).

//! The web's adapter: the tree, mirrored as elements over the window's
//! canvas, which the browser takes into its own accessibility tree.
//!
//! A canvas is one box to a browser: what is drawn on it has no elements,
//! and so no roles, names or states for assistive tech to read. So each node
//! of the tree is an element here, with the ARIA role, name and states of
//! its node, nested as the tree nests and laid over the canvas at the node's
//! bounds, so a screen reader that explores by touch finds it where it is
//! drawn. The elements are transparent and take no pointer events: a mouse
//! still reaches the canvas, and nothing is seen twice.
//!
//! The keyboard stays with the canvas, where winit listens: the canvas is an
//! `application` that owns the mirror (`aria-owns`), and the focused node is
//! its active descendant (`aria-activedescendant`), which a screen reader
//! announces as focus moves. What assistive tech does to an element, a click
//! (which a screen reader sends when the user activates it), comes back as
//! the node's action: a click, or a focus for a node that can only be
//! focused, such as a text field.
//!
//! The tree an app sends is whole or a change to it: each node it names is
//! updated, and an element no longer reached from the root is removed. An
//! element is touched only where its node changed, as the tree is sent again
//! for every frame drawn.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId,
    Rect, Role, Toggled, TreeId, TreeUpdate,
};
use raw_window_handle::RawWindowHandle;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement};

/// The attribute that names the node an element mirrors.
const NODE_ATTR: &str = "data-accesskit-node";
/// The class of the element that holds the mirror.
const ROOT_CLASS: &str = "accesskit-mirror";

/// How the mirror is kept out of sight and out of the pointer's way, once for
/// the page. Nothing is clipped: a node with no bounds (the shell's window)
/// is an element of no size, and what it holds is still where it is drawn.
const STYLE: &str = ".accesskit-mirror, .accesskit-mirror * {\
    position: absolute; box-sizing: border-box; margin: 0; padding: 0; border: 0;\
    overflow: visible; color: transparent; background: transparent;\
    pointer-events: none; user-select: none; -webkit-user-select: none;\
    caret-color: transparent; outline: none; white-space: pre; font-size: 1px;\
}\
.accesskit-mirror { position: fixed; z-index: 2147483647; }";

pub struct Adapter {
    state: Rc<State>,
}

struct State {
    document: Document,
    canvas: HtmlCanvasElement,
    /// The element that holds the mirror, laid over the canvas.
    root: HtmlElement,
    mirror: RefCell<Mirror>,
    actions: RefCell<Box<dyn ActionHandler>>,
    /// The listener for what assistive tech does to an element, kept for as
    /// long as the adapter lives.
    _click: Closure<dyn FnMut(web_sys::Event)>,
}

#[derive(Default)]
struct Mirror {
    nodes: HashMap<NodeId, Mirrored>,
    root: Option<NodeId>,
    focus: Option<NodeId>,
    host_focused: bool,
}

struct Mirrored {
    node: Node,
    element: HtmlElement,
    /// The box last given to the element: left, top, width and height in
    /// CSS pixels, relative to its parent's.
    placed: Option<[f64; 4]>,
}

impl Adapter {
    pub fn new(
        window_handle: RawWindowHandle,
        mut activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let RawWindowHandle::WebCanvas(handle) = window_handle else {
            panic!("accesskit_xplat on the web takes a canvas's window handle");
        };
        // SAFETY: winit's handle points at the `JsValue` of the window's
        // canvas, which lives as long as the window; the canvas is cloned out
        // of it here, so the adapter keeps its own reference.
        let canvas: HtmlCanvasElement =
            unsafe { handle.obj.cast::<wasm_bindgen::JsValue>().as_ref() }
                .clone()
                .unchecked_into();
        let document = canvas
            .owner_document()
            .expect("the window's canvas is in a document");

        if document
            .query_selector("style[data-accesskit-mirror]")
            .ok()
            .flatten()
            .is_none()
        {
            let style = document.create_element("style").unwrap();
            style.set_attribute("data-accesskit-mirror", "").unwrap();
            style.set_text_content(Some(STYLE));
            if let Some(head) = document.head() {
                head.append_child(&style).unwrap();
            }
        }

        let root: HtmlElement = document.create_element("div").unwrap().unchecked_into();
        root.set_class_name(ROOT_CLASS);
        let root_id = format!("accesskit-mirror-{}", next_mirror_number());
        root.set_id(&root_id);
        match canvas.parent_node() {
            Some(parent) => {
                parent
                    .insert_before(&root, canvas.next_sibling().as_ref())
                    .unwrap();
            }
            None => {
                if let Some(body) = document.body() {
                    body.append_child(&root).unwrap();
                }
            }
        }
        canvas.set_attribute("role", "application").unwrap();
        canvas.set_attribute("aria-owns", &root_id).unwrap();

        let actions: RefCell<Box<dyn ActionHandler>> = RefCell::new(Box::new(action_handler));
        let state = Rc::new_cyclic(|weak: &std::rc::Weak<State>| {
            let weak = weak.clone();
            let click = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                if let Some(state) = weak.upgrade() {
                    state.clicked(&event);
                }
            });
            root.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())
                .unwrap();
            State {
                document,
                canvas,
                root,
                mirror: RefCell::default(),
                actions,
                _click: click,
            }
        });

        // A browser cannot say whether assistive tech is listening, so the
        // tree is always wanted.
        if let Some(tree) = activation_handler.request_initial_tree() {
            state.update(tree);
        }
        Self { state }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.state.update(updater());
    }

    pub fn set_focus(&mut self, is_focused: bool) {
        let mut mirror = self.state.mirror.borrow_mut();
        mirror.host_focused = is_focused;
        self.state.show_focus(&mirror);
    }

    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {
        self.state.place_root();
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.state.root.remove();
        let _ = self.state.canvas.remove_attribute("aria-owns");
        let _ = self.state.canvas.remove_attribute("aria-activedescendant");
    }
}

impl State {
    fn update(&self, update: TreeUpdate) {
        if update.tree_id != TreeId::ROOT {
            return;
        }
        let mut mirror = self.mirror.borrow_mut();
        if let Some(tree) = &update.tree {
            mirror.root = Some(tree.root);
        }
        for (id, node) in update.nodes {
            match mirror.nodes.get_mut(&id) {
                // A node keeps its element while its role does: what one
                // role writes (a slider's value, a field's text) is not
                // another's to clear.
                Some(m) if m.node.role() == node.role() => {
                    if m.node != node {
                        describe(&m.element, &node, Some(&m.node));
                        m.node = node;
                    }
                }
                other => {
                    let element: HtmlElement = self
                        .document
                        .create_element(tag_for(node.role()))
                        .unwrap()
                        .unchecked_into();
                    element.set_id(&element_id(&self.root, id));
                    element.set_attribute(NODE_ATTR, &id.0.to_string()).unwrap();
                    describe(&element, &node, None);
                    if let Some(old) = other {
                        old.element.replace_with_with_node_1(&element).ok();
                    }
                    mirror.nodes.insert(
                        id,
                        Mirrored {
                            node,
                            element,
                            placed: None,
                        },
                    );
                }
            }
        }
        mirror.focus = Some(update.focus);

        self.place_root();
        let dpr = self
            .document
            .default_view()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| *r > 0.0)
            .unwrap_or(1.0);
        let mut reached = HashSet::new();
        if let Some(root) = mirror.root {
            self.arrange(&mut mirror, root, &self.root, [0.0, 0.0], dpr, &mut reached);
        }
        mirror.nodes.retain(|id, m| {
            let keep = reached.contains(id);
            if !keep {
                m.element.remove();
            }
            keep
        });
        self.show_focus(&mirror);
    }

    /// Puts node `id`'s element in `parent` and its children's in its own, in
    /// the tree's order, each at its bounds. `origin` is the parent's top
    /// left in CSS pixels.
    fn arrange(
        &self,
        mirror: &mut Mirror,
        id: NodeId,
        parent: &Element,
        origin: [f64; 2],
        dpr: f64,
        reached: &mut HashSet<NodeId>,
    ) {
        if !reached.insert(id) {
            return;
        }
        let Some(m) = mirror.nodes.get_mut(&id) else {
            return;
        };
        let element = m.element.clone();
        if element.parent_element().as_ref() != Some(parent) {
            parent.append_child(&element).unwrap();
        }
        let placed = match m.node.bounds() {
            Some(b) => [
                b.x0 / dpr - origin[0],
                b.y0 / dpr - origin[1],
                (b.x1 - b.x0) / dpr,
                (b.y1 - b.y0) / dpr,
            ],
            None => [0.0, 0.0, 0.0, 0.0],
        };
        if m.placed != Some(placed) {
            let style = element.style();
            let _ = style.set_property("left", &format!("{}px", placed[0]));
            let _ = style.set_property("top", &format!("{}px", placed[1]));
            let _ = style.set_property("width", &format!("{}px", placed[2]));
            let _ = style.set_property("height", &format!("{}px", placed[3]));
            m.placed = Some(placed);
        }
        let origin = [origin[0] + placed[0], origin[1] + placed[1]];
        // A field's text is its value, written into its element: the runs
        // under it are its editing model, not more content.
        if takes_value_as_text(m.node.role()) {
            return;
        }
        let children: Vec<NodeId> = m.node.children().to_vec();
        let mut at = element.first_element_child();
        for child in children {
            let Some(child_element) = mirror.nodes.get(&child).map(|c| c.element.clone()) else {
                continue;
            };
            // A run is its label's text, already written: kept in the
            // mirror, out of the page.
            if mirror.nodes[&child].node.role() == Role::TextRun {
                reached.insert(child);
                if child_element.parent_element().is_some() {
                    if at
                        .as_ref()
                        .is_some_and(|e| e == AsRef::<Element>::as_ref(&child_element))
                    {
                        at = child_element.next_element_sibling();
                    }
                    child_element.remove();
                }
                continue;
            }
            match &at {
                Some(e) if e == AsRef::<Element>::as_ref(&child_element) => {
                    at = e.next_element_sibling()
                }
                _ => {
                    element
                        .insert_before(&child_element, at.as_ref().map(|e| e.as_ref()))
                        .unwrap();
                }
            }
            self.arrange(mirror, child, &element, origin, dpr, reached);
        }
        // Elements left after the children are of nodes no longer under this
        // one: they move when their new parent is arranged, or go with the
        // nodes no longer reached.
    }

    /// Lays the mirror over the canvas, as the canvas is now on the page.
    fn place_root(&self) {
        let rect = self.canvas.get_bounding_client_rect();
        let style = self.root.style();
        let _ = style.set_property("left", &format!("{}px", rect.left()));
        let _ = style.set_property("top", &format!("{}px", rect.top()));
        let _ = style.set_property("width", &format!("{}px", rect.width()));
        let _ = style.set_property("height", &format!("{}px", rect.height()));
    }

    fn show_focus(&self, mirror: &Mirror) {
        let focused = mirror
            .focus
            .filter(|f| Some(*f) != mirror.root)
            .and_then(|f| mirror.nodes.get(&f));
        match focused {
            Some(m) => {
                let _ = self
                    .canvas
                    .set_attribute("aria-activedescendant", &m.element.id());
            }
            None => {
                let _ = self.canvas.remove_attribute("aria-activedescendant");
            }
        }
    }

    /// What assistive tech did to an element: its node's click, or its focus
    /// where it has no click. The canvas takes the keyboard again, for what
    /// is typed next.
    fn clicked(&self, event: &web_sys::Event) {
        let Some(target) = event
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|e| e.closest(&format!("[{NODE_ATTR}]")).ok().flatten())
        else {
            return;
        };
        let Some(id) = target
            .get_attribute(NODE_ATTR)
            .and_then(|n| n.parse::<u64>().ok())
            .map(NodeId)
        else {
            return;
        };
        let action = {
            let mirror = self.mirror.borrow();
            let Some(m) = mirror.nodes.get(&id) else {
                return;
            };
            if m.node.supports_action(Action::Click) {
                Action::Click
            } else if m.node.supports_action(Action::Focus) {
                Action::Focus
            } else {
                return;
            }
        };
        event.prevent_default();
        let _ = self.canvas.focus();
        self.actions.borrow_mut().do_action(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: None,
        });
    }
}

thread_local! {
    static MIRRORS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A number for each mirror made on the page, so two windows' element ids
/// differ.
fn next_mirror_number() -> u64 {
    MIRRORS.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

fn element_id(root: &HtmlElement, id: NodeId) -> String {
    format!("{}-{}", root.id(), id.0)
}

/// The element a node is mirrored as: a `span` for text, a `div` otherwise.
fn tag_for(role: Role) -> &'static str {
    match role {
        Role::Label | Role::TextRun => "span",
        _ => "div",
    }
}

/// Whether a node's value is its text: a text field's.
fn takes_value_as_text(role: Role) -> bool {
    matches!(
        role,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::EmailInput
            | Role::PasswordInput
            | Role::PhoneNumberInput
            | Role::UrlInput
            | Role::NumberInput
            | Role::DateInput
            | Role::DateTimeInput
            | Role::WeekInput
            | Role::MonthInput
            | Role::TimeInput
    )
}

/// Writes `node`'s role, name and states onto `element`. `old` is the node
/// the element last mirrored, whose attributes are cleared where `node` has
/// none.
fn describe(element: &HtmlElement, node: &Node, old: Option<&Node>) {
    let set = |name: &str, value: Option<String>| match value {
        Some(v) => {
            if element.get_attribute(name).as_deref() != Some(v.as_str()) {
                let _ = element.set_attribute(name, &v);
            }
        }
        None => {
            if old.is_some() {
                let _ = element.remove_attribute(name);
            }
        }
    };
    let role = node.role();
    set("role", aria_role(role).map(str::to_owned));
    set("aria-label", node.label().map(str::to_owned));
    set("aria-description", node.description().map(str::to_owned));
    set(
        "aria-roledescription",
        node.role_description().map(str::to_owned),
    );
    set("aria-placeholder", node.placeholder().map(str::to_owned));
    set("aria-hidden", node.is_hidden().then(|| "true".to_owned()));
    set(
        "aria-disabled",
        node.is_disabled().then(|| "true".to_owned()),
    );
    set(
        "aria-readonly",
        node.is_read_only().then(|| "true".to_owned()),
    );
    set(
        "aria-required",
        node.is_required().then(|| "true".to_owned()),
    );
    set("aria-modal", node.is_modal().then(|| "true".to_owned()));
    set(
        "aria-multiline",
        (role == Role::MultilineTextInput).then(|| "true".to_owned()),
    );
    set(
        "aria-multiselectable",
        node.is_multiselectable().then(|| "true".to_owned()),
    );
    set("aria-expanded", node.is_expanded().map(|e| e.to_string()));
    set("aria-selected", node.is_selected().map(|s| s.to_string()));
    let toggled = node.toggled().map(|t| {
        match t {
            Toggled::True => "true",
            Toggled::False => "false",
            Toggled::Mixed => "mixed",
        }
        .to_owned()
    });
    let pressed = role == Role::Button;
    set("aria-pressed", toggled.clone().filter(|_| pressed));
    set("aria-checked", toggled.filter(|_| !pressed));
    set("aria-level", node.level().map(|l| (l + 1).to_string()));
    if matches!(
        role,
        Role::Slider | Role::SpinButton | Role::ProgressIndicator | Role::ScrollBar
    ) {
        set("aria-valuenow", node.numeric_value().map(|v| v.to_string()));
        set(
            "aria-valuemin",
            node.min_numeric_value().map(|v| v.to_string()),
        );
        set(
            "aria-valuemax",
            node.max_numeric_value().map(|v| v.to_string()),
        );
        set("aria-valuetext", node.value().map(str::to_owned));
    }

    // Text is the element's own: a label's, which by AccessKit's convention
    // is its value, and a field's, its value. A password's value is already
    // its bullets.
    let text = match role {
        Role::Label | Role::TextRun => node.value().or(node.label()),
        r if takes_value_as_text(r) => node.value(),
        _ => None,
    };
    if role == Role::Label || role == Role::TextRun || takes_value_as_text(role) {
        let text = text.unwrap_or("");
        if element.text_content().as_deref() != Some(text) {
            element.set_text_content(Some(text));
        }
    }
}

/// The ARIA role a node is mirrored with: none where the element's own
/// (a `div`, a `span`) says it, a generic container or text.
fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Alert => "alert",
        Role::AlertDialog => "alertdialog",
        Role::Application => "application",
        Role::Article => "article",
        Role::Banner | Role::Header => "banner",
        Role::Blockquote => "blockquote",
        Role::Button | Role::DefaultButton => "button",
        Role::Caption => "caption",
        Role::Cell => "cell",
        Role::CheckBox => "checkbox",
        Role::Code => "code",
        Role::ColumnHeader => "columnheader",
        Role::ComboBox | Role::EditableComboBox => "combobox",
        Role::Complementary => "complementary",
        Role::ContentInfo | Role::Footer => "contentinfo",
        Role::Definition => "definition",
        Role::Dialog => "dialog",
        Role::Document => "document",
        Role::Emphasis => "emphasis",
        Role::Figure => "figure",
        Role::Form => "form",
        Role::Grid => "grid",
        Role::GridCell => "gridcell",
        Role::Group => "group",
        Role::Heading => "heading",
        Role::Image => "img",
        Role::Link => "link",
        Role::List => "list",
        Role::ListBox => "listbox",
        Role::ListBoxOption => "option",
        Role::ListItem => "listitem",
        Role::Log => "log",
        Role::Main => "main",
        Role::Mark => "mark",
        Role::Marquee => "marquee",
        Role::Math => "math",
        Role::Menu => "menu",
        Role::MenuBar => "menubar",
        Role::MenuItem => "menuitem",
        Role::MenuItemCheckBox => "menuitemcheckbox",
        Role::MenuItemRadio => "menuitemradio",
        Role::Meter => "meter",
        Role::Navigation => "navigation",
        Role::Note => "note",
        Role::Paragraph => "paragraph",
        Role::ProgressIndicator => "progressbar",
        Role::RadioButton => "radio",
        Role::RadioGroup => "radiogroup",
        Role::Region => "region",
        Role::Row => "row",
        Role::RowGroup => "rowgroup",
        Role::RowHeader => "rowheader",
        Role::ScrollBar => "scrollbar",
        Role::Search => "search",
        Role::SearchInput => "searchbox",
        Role::Splitter => "separator",
        Role::Slider => "slider",
        Role::SpinButton | Role::NumberInput => "spinbutton",
        Role::Status => "status",
        Role::Strong => "strong",
        Role::Switch => "switch",
        Role::Tab => "tab",
        Role::TabList => "tablist",
        Role::TabPanel => "tabpanel",
        Role::Table => "table",
        Role::Term => "term",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::EmailInput
        | Role::PasswordInput
        | Role::PhoneNumberInput
        | Role::UrlInput
        | Role::DateInput
        | Role::DateTimeInput
        | Role::WeekInput
        | Role::MonthInput
        | Role::TimeInput => "textbox",
        Role::Time => "time",
        Role::Timer => "timer",
        Role::Toolbar => "toolbar",
        Role::Tooltip => "tooltip",
        Role::Tree => "tree",
        Role::TreeGrid => "treegrid",
        Role::TreeItem => "treeitem",
        _ => return None,
    })
}
