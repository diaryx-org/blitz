//! PATCH(diaryx): `<input type=file>`.
//!
//! A press on the input asks the shell for the system's picker
//! ([`ShellProvider::pick_files`]) with what the input's `accept`,
//! `multiple` and `webkitdirectory` ask for, and returns: the window keeps
//! drawing while the picker is open. The answer comes back as
//! [`UiEvent::FilesChosen`](blitz_traits::events::UiEvent::FilesChosen),
//! which [`BaseDocument::files_chosen`] turns into the input's files, its
//! label, and the events a browser fires: `input` and `change` for files
//! chosen, `cancel` for none.
//!
//! A shell with no picker that answers later is asked for one that answers
//! at once ([`ShellProvider::open_file_dialog`]), as before.
//!
//! [`ShellProvider::pick_files`]: blitz_traits::shell::ShellProvider::pick_files
//! [`ShellProvider::open_file_dialog`]: blitz_traits::shell::ShellProvider::open_file_dialog

use blitz_traits::events::{BlitzInputEvent, DomEvent, DomEventData};
use blitz_traits::files::{ChosenFile, FileRequest};
use blitz_traits::shell::FileDialogFilter;
use markup5ever::{LocalName, local_name};

use crate::node::SpecialElementData;
use crate::{BaseDocument, NodeId, qual_name};

/// What the file input `node_id` asks the picker for.
pub(crate) fn request(doc: &BaseDocument, node_id: NodeId) -> FileRequest {
    let Some(el) = doc.nodes[node_id].element_data() else {
        return FileRequest::default();
    };
    FileRequest::from_attributes(
        el.attr(local_name!("accept")),
        el.attr(local_name!("multiple")).is_some(),
        el.attr(LocalName::from("webkitdirectory")).is_some(),
    )
}

/// A press on the file input `node_id`: the system's picker.
pub(crate) fn open_picker(
    doc: &mut BaseDocument,
    node_id: NodeId,
    dispatch_event: &mut dyn FnMut(DomEvent),
) {
    let request = request(doc, node_id);
    if doc.shell_provider.pick_files(node_id, request.clone()) {
        return;
    }
    // A shell whose picker answers at once.
    let extensions = request.extensions();
    let filter = (!extensions.is_empty()).then(|| FileDialogFilter {
        name: request.filter_name(),
        extensions,
    });
    let paths = doc
        .shell_provider
        .open_file_dialog(request.multiple, filter);
    let files: Vec<ChosenFile> = paths
        .into_iter()
        .flat_map(|p| {
            if request.directory && p.is_dir() {
                ChosenFile::from_folder(&p)
            } else {
                ChosenFile::from_path(p).into_iter().collect()
            }
        })
        .collect();
    let answer = (!files.is_empty()).then_some(files);
    for event in doc.files_chosen(node_id, answer) {
        dispatch_event(event);
    }
}

/// What a file input says it holds, as a browser's says it.
fn label_for(files: &[ChosenFile], directory: bool) -> String {
    match files {
        [] if directory => "No folder chosen".into(),
        [] => "No file chosen".into(),
        [one] => one.name.clone(),
        many => format!("{} files", many.len()),
    }
}

impl BaseDocument {
    /// The picker's answer for the file input `node_id`: its files (`None`
    /// if it was cancelled), and the events to dispatch for it, in order.
    ///
    /// Files a picker could not filter by type that `accept` does not allow
    /// are left out, as a browser leaves them out. Files chosen replace the
    /// input's, and its `value` becomes the first's name under the path a
    /// browser gives every file (`C:\fakepath\`), so that the page never
    /// learns where a file was; the files themselves carry that, through the
    /// `input` and `change` events. A cancel leaves the files as they were
    /// and fires `cancel`.
    pub fn files_chosen(
        &mut self,
        node_id: NodeId,
        files: Option<Vec<ChosenFile>>,
    ) -> Vec<DomEvent> {
        let is_file_input = self.nodes.get(node_id).is_some_and(|n| {
            n.element_data().is_some_and(|el| {
                el.name.local == local_name!("input")
                    && el.attr(local_name!("type")) == Some("file")
            })
        });
        if !is_file_input {
            return Vec::new();
        }
        let request = request(self, node_id);
        let files: Option<Vec<ChosenFile>> = files
            .map(|f| {
                f.into_iter()
                    .filter(|f| request.accepts(f))
                    .collect::<Vec<_>>()
            })
            .filter(|f| !f.is_empty())
            .map(|mut f| {
                if !request.multiple && !request.directory {
                    f.truncate(1);
                }
                f
            });
        let Some(files) = files else {
            let value = self.nodes[node_id]
                .attr(local_name!("value"))
                .unwrap_or_default()
                .to_string();
            return vec![DomEvent::new(
                node_id,
                DomEventData::Cancel(BlitzInputEvent {
                    value,
                    files: Vec::new(),
                }),
            )];
        };

        let value = format!("C:\\fakepath\\{}", files[0].name);
        let label = label_for(&files, request.directory);
        let node = &mut self.nodes[node_id];
        let el = node.element_data_mut().expect("a file input is an element");
        el.attrs.set(qual_name!("value", html), &value);
        el.special_data = SpecialElementData::FileInput(files.clone().into());
        let label_id = node.children.get(1).copied();

        // The label the input drew for itself: its second child, a <label>
        // holding a text node (made with the input, in the mutator).
        let text_id = label_id.and_then(|label| self.nodes[label].children.first().copied());
        if let Some(text) = text_id.and_then(|id| self.nodes[id].text_data_mut()) {
            text.content = label;
        }
        self.nodes[node_id].mark_damaged();
        self.shell_provider.request_redraw();

        let data = BlitzInputEvent { value, files };
        vec![
            DomEvent::new(node_id, DomEventData::Input(data.clone())),
            DomEvent::new(node_id, DomEventData::Change(data)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use blitz_traits::events::{DomEvent, DomEventData, EventState, UiEvent};
    use blitz_traits::files::{ChosenFile, FileRequest};
    use blitz_traits::node_id::NodeId;
    use blitz_traits::shell::ShellProvider;

    use crate::node::Attribute;
    use crate::{BaseDocument, Document, DocumentConfig, EventDriver, EventHandler, qual_name};

    /// A shell whose picker records what it was asked for, and answers when
    /// the test says.
    #[derive(Default)]
    struct Picker(Mutex<Vec<(NodeId, FileRequest)>>);
    impl ShellProvider for Picker {
        fn pick_files(&self, target: NodeId, request: FileRequest) -> bool {
            self.0.lock().unwrap().push((target, request));
            true
        }
    }

    /// What the page heard: each event's name, value and files' names.
    #[derive(Clone, Default)]
    struct Heard(Arc<Mutex<Vec<Said>>>);

    /// An event's name, its value, and its files' names.
    type Said = (String, String, Vec<String>);
    impl EventHandler for Heard {
        fn handle_event(
            &mut self,
            _chain: &[NodeId],
            event: &mut DomEvent,
            _doc: &mut dyn Document,
            _state: &mut EventState,
        ) {
            if let DomEventData::Input(d) | DomEventData::Change(d) | DomEventData::Cancel(d) =
                &event.data
            {
                self.0.lock().unwrap().push((
                    event.data.name().to_string(),
                    d.value.clone(),
                    d.files.iter().map(|f| f.name.clone()).collect(),
                ));
            }
        }
    }

    fn page(attrs: &[(&str, &str)]) -> (BaseDocument, Arc<Picker>, NodeId) {
        let picker = Arc::new(Picker::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            shell_provider: Some(picker.clone()),
            ..Default::default()
        });
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html"), vec![]);
        let mut all = vec![Attribute {
            name: qual_name!("type", html),
            value: "file".into(),
        }];
        for (k, v) in attrs {
            all.push(Attribute {
                name: crate::QualName {
                    prefix: None,
                    ns: crate::ns!(html),
                    local: markup5ever::LocalName::from(*k),
                },
                value: (*v).into(),
            });
        }
        let input = m.create_element(qual_name!("input", html), all);
        m.append_children(html, &[input]);
        m.append_children(root, &[html]);
        drop(m);
        (doc, picker, input)
    }

    fn file(name: &str) -> ChosenFile {
        ChosenFile::from_bytes(name, b"x".to_vec().into(), 0)
    }

    fn answer(doc: &mut BaseDocument, input: NodeId, files: Option<Vec<ChosenFile>>) -> Heard {
        let heard = Heard::default();
        let mut driver = EventDriver::new(doc as &mut dyn Document, heard.clone());
        driver.handle_ui_event(UiEvent::FilesChosen {
            target: input,
            files,
        });
        heard
    }

    fn label(doc: &BaseDocument, input: NodeId) -> String {
        let label = doc.nodes[input].children[1];
        doc.nodes[label].text_content()
    }

    #[test]
    fn a_press_asks_the_picker_for_what_the_input_asks_for() {
        let (mut doc, picker, input) = page(&[("accept", ".md,image/*"), ("multiple", "")]);
        let mut events = Vec::new();
        super::open_picker(&mut doc, input, &mut |e| events.push(e));
        assert!(events.is_empty(), "nothing yet: the picker answers later");
        let asked = picker.0.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        let (target, request) = &asked[0];
        assert_eq!(*target, input);
        assert!(request.multiple && !request.directory);
        assert_eq!(request.extensions()[0], "md");
        assert!(request.extensions().contains(&"png".to_string()));
        assert_eq!(label(&doc, input), "No file chosen");
    }

    #[test]
    fn files_chosen_are_the_inputs_and_fire_input_then_change() {
        let (mut doc, _, input) = page(&[("multiple", "")]);
        let heard = answer(&mut doc, input, Some(vec![file("a.md"), file("b.png")]));
        let heard = heard.0.lock().unwrap().clone();
        let names: Vec<_> = heard.iter().map(|h| h.0.as_str()).collect();
        assert_eq!(names, ["input", "change"]);
        assert_eq!(heard[0].1, "C:\\fakepath\\a.md", "a browser's value");
        assert_eq!(heard[1].2, ["a.md", "b.png"]);
        let held: Vec<_> = doc.nodes[input]
            .element_data()
            .unwrap()
            .file_data()
            .unwrap()
            .iter()
            .map(|f| f.name.clone())
            .collect();
        assert_eq!(held, ["a.md", "b.png"]);
        assert_eq!(label(&doc, input), "2 files");
    }

    #[test]
    fn one_input_keeps_one_file_of_the_types_it_accepts() {
        let (mut doc, _, input) = page(&[("accept", ".kdbx")]);
        let heard = answer(
            &mut doc,
            input,
            Some(vec![
                file("notes.txt"),
                file("Passwords.kdbx"),
                file("Old.kdbx"),
            ]),
        );
        assert_eq!(heard.0.lock().unwrap()[1].2, ["Passwords.kdbx"]);
        assert_eq!(label(&doc, input), "Passwords.kdbx");
    }

    #[test]
    fn return_or_space_on_the_focused_input_opens_its_picker() {
        use blitz_traits::events::{BlitzKeyEvent, KeyState};
        use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
        let (mut doc, picker, input) = page(&[("accept", ".md")]);
        doc.set_focus_to(input);
        let press = |key: Key| {
            UiEvent::KeyDown(BlitzKeyEvent {
                key,
                code: Code::Unidentified,
                modifiers: Modifiers::empty(),
                location: Location::Standard,
                is_auto_repeating: false,
                is_composing: false,
                state: KeyState::Pressed,
                text: None,
            })
        };
        for key in [Key::Named(NamedKey::Enter), Key::Character(" ".into())] {
            let mut driver = EventDriver::new(&mut doc as &mut dyn Document, Heard::default());
            driver.handle_ui_event(press(key));
        }
        let asked = picker.0.lock().unwrap();
        assert_eq!(asked.len(), 2, "a picker for each key");
        assert!(
            asked
                .iter()
                .all(|(t, r)| *t == input && r.extensions() == ["md"])
        );
    }

    #[test]
    fn a_cancel_keeps_the_files_and_fires_cancel() {
        let (mut doc, _, input) = page(&[]);
        answer(&mut doc, input, Some(vec![file("a.md")]));
        let heard = answer(&mut doc, input, None);
        let heard = heard.0.lock().unwrap().clone();
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].0, "cancel");
        assert_eq!(label(&doc, input), "a.md", "the file chosen before stays");
        // Nothing of a type it accepts is a cancel too.
        let (mut doc, _, input) = page(&[("accept", "image/*")]);
        let heard = answer(&mut doc, input, Some(vec![file("a.md")]));
        assert_eq!(heard.0.lock().unwrap()[0].0, "cancel");
    }
}
