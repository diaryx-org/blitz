//! PATCH(diaryx): the browser's picker for a file input, when Blitz draws a
//! page into a canvas: the page's `<input type=file>` exists only in Blitz,
//! so the browser is given one of its own, hidden, as the IME is given a
//! hidden textarea (winit-web's patch), and clicked while the press that
//! asked still lets a page open a picker.
//!
//! - `accept`, `multiple` and `webkitdirectory` are the input's own, so the
//!   browser filters and chooses as it would for any page.
//! - The files are read whole (`File.arrayBuffer`) and handed over as bytes,
//!   with their names, types, times, and their paths within a folder chosen
//!   (`webkitRelativePath`): a browser gives a page no paths.
//! - A picker dismissed is the input's `cancel` event; a browser too old to
//!   fire it leaves the input with no answer.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use blitz_traits::files::{Accept, ChosenFile, FileRequest};
use blitz_traits::node_id::NodeId;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, HtmlInputElement};
use winit::window::Window;

use crate::BlitzShellProxy;

/// `request`'s `accept`, written back as an attribute.
fn accept_attr(request: &FileRequest) -> String {
    request
        .accept
        .iter()
        .map(|a| match a {
            Accept::Extension(e) => format!(".{e}"),
            Accept::Mime(m) => m.clone(),
            Accept::Group(g) => format!("{g}/*"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// A browser's file, read whole.
async fn read(file: File) -> Option<ChosenFile> {
    let buffer = JsFuture::from(file.array_buffer()).await.ok()?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    let mut chosen = ChosenFile::from_bytes(file.name(), bytes.into(), file.last_modified() as u64);
    // Not in web-sys: read by name.
    let relative = js_sys::Reflect::get(&file, &"webkitRelativePath".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    if !relative.is_empty() {
        chosen.relative_path = relative;
    }
    let kind = file.type_();
    if !kind.is_empty() {
        chosen.content_type = Some(kind);
    }
    Some(chosen)
}

pub(super) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };
    let Some(body) = document.body() else {
        return false;
    };
    let Ok(input) = document
        .create_element("input")
        .map(|e| e.unchecked_into::<HtmlInputElement>())
    else {
        return false;
    };
    input.set_type("file");
    input.set_accept(&accept_attr(&request));
    input.set_multiple(request.multiple);
    if request.directory {
        let _ = input.set_attribute("webkitdirectory", "");
    }
    // Out of sight and out of the way: not displayed, which some browsers
    // would refuse to open a picker for, but nowhere to be seen or pressed.
    let _ = input.set_attribute(
        "style",
        "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none",
    );
    let _ = input.set_attribute("aria-hidden", "true");
    let _ = input.set_attribute("tabindex", "-1");
    if body.append_child(&input).is_err() {
        return false;
    }

    // Answered once: by `change` or by `cancel`, whichever comes.
    let answered = Rc::new(Cell::new(false));
    let on_change = {
        let input = input.clone();
        let answered = answered.clone();
        let proxy = proxy.clone();
        let window = window.clone();
        Closure::<dyn FnMut()>::new(move || {
            if answered.replace(true) {
                return;
            }
            let files: Vec<File> = input
                .files()
                .map(|list| (0..list.length()).filter_map(|i| list.get(i)).collect())
                .unwrap_or_default();
            input.remove();
            let proxy = proxy.clone();
            let window = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut chosen = Vec::new();
                for f in files {
                    chosen.extend(read(f).await);
                }
                #[cfg(feature = "tracing")]
                tracing::info!(
                    "file input {target:?}: {:?}",
                    chosen.iter().map(|f| (&f.name, f.size)).collect::<Vec<_>>()
                );
                super::answer(
                    &proxy,
                    &*window,
                    target,
                    (!chosen.is_empty()).then_some(chosen),
                );
            });
        })
    };
    let on_cancel = {
        let input = input.clone();
        let answered = answered.clone();
        let proxy = proxy.clone();
        let window = window.clone();
        Closure::<dyn FnMut()>::new(move || {
            if answered.replace(true) {
                return;
            }
            input.remove();
            super::answer(&proxy, &*window, target, None);
        })
    };
    let _ = input.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
    let _ = input.add_event_listener_with_callback("cancel", on_cancel.as_ref().unchecked_ref());
    // The listeners live as long as the input; each is called at most once.
    on_change.forget();
    on_cancel.forget();
    input.click();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_is_written_back() {
        let r = FileRequest::from_attributes(Some(" .KDBX, image/*,application/pdf"), true, false);
        assert_eq!(accept_attr(&r), ".kdbx,image/*,application/pdf");
    }
}
