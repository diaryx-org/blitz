//! PATCH(diaryx): the system's file picker, for `<input type=file>`.
//!
//! [`pick`] shows it for a file input and returns at once; the picker's
//! answer reaches the window as [`BlitzShellEvent::FilesChosen`], which the
//! application hands to the document. The window keeps drawing while the
//! picker is open, as a browser's does.
//!
//! Each platform's picker is written here, against the system's own API,
//! with the same defaults: filtered by the input's `accept` (any file only
//! when `accept` allows any), opening in the folder the last file was chosen
//! from, attached to the window that asked.
//!
//! - **macOS** (`macos.rs`): AppKit's open panel, a sheet on the window.
//! - **Windows** (`windows.rs`): the Common Item Dialog, the window's
//!   child, through COM interfaces declared by hand.
//! - **Linux and the BSDs** (`linux.rs`): the desktop's own picker, through
//!   the XDG desktop portal over D-Bus (`dbus.rs`, a client of our own), or
//!   zenity or kdialog where there is no portal.
//! - **iOS** (`ios.rs`): UIKit's document picker over the window.
//! - **The web** (`web.rs`), Blitz drawing into a canvas: the browser's own,
//!   through a hidden `<input type=file>` of the page's, its files read whole.
//!
//! Elsewhere there is no picker, and the document falls back to
//! `ShellProvider::open_file_dialog`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use blitz_traits::files::{ChosenFile, FileRequest};
use blitz_traits::node_id::NodeId;
use winit::window::Window;

use crate::{BlitzShellEvent, BlitzShellProxy};

/// Where a picker's answer goes, once: the files chosen, or `None` for a
/// cancel (or a picker that failed). To a file input ([`pick`]), or to code
/// waiting for it ([`choose_files`]).
pub(crate) type Answer = Box<dyn FnOnce(Option<Vec<ChosenFile>>) + Send>;

/// The folder the last file was chosen from, which the next picker opens
/// in, as a browser's does.
static LAST_FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

#[allow(dead_code)]
fn last_folder() -> Option<PathBuf> {
    LAST_FOLDER.lock().ok().and_then(|f| f.clone())
}

/// The files a picker's paths are, its folder remembered for the next: a
/// folder chosen for an input that chooses one (`webkitdirectory`) is its
/// files, every one under it.
#[allow(dead_code)]
fn chosen(paths: Vec<PathBuf>, directory: bool) -> Vec<ChosenFile> {
    let folder = if directory {
        paths.first().cloned()
    } else {
        paths.first().and_then(|p| p.parent().map(PathBuf::from))
    };
    if let (Some(folder), Ok(mut last)) = (folder, LAST_FOLDER.lock()) {
        *last = Some(folder);
    }
    paths
        .into_iter()
        .flat_map(|p| {
            if directory {
                ChosenFile::from_folder(&p)
            } else {
                ChosenFile::from_path(p).into_iter().collect()
            }
        })
        .collect()
}

/// Run `pick` on a thread of its own, where it may wait on the system for
/// as long as the person takes, and `answer` with what it gives: `None` for
/// a cancel (or a picker that failed).
#[allow(dead_code)]
fn on_thread(
    directory: bool,
    pick: impl FnOnce() -> Option<Vec<PathBuf>> + Send + 'static,
    answer: Answer,
) -> bool {
    std::thread::Builder::new()
        .name("blitz-file-picker".into())
        .spawn(move || {
            let files = pick()
                .map(|paths| chosen(paths, directory))
                .filter(|f| !f.is_empty());
            answer(files);
        })
        .is_ok()
}

#[cfg(all(feature = "file-dialog", target_os = "windows"))]
mod windows;

#[cfg(all(
    feature = "file-dialog",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
mod dbus;
#[cfg(all(
    feature = "file-dialog",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
mod linux;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(all(feature = "file-dialog", target_os = "macos"))]
mod macos;
#[cfg(any(target_os = "ios", all(feature = "file-dialog", target_os = "macos")))]
mod uttypes;
#[cfg(all(feature = "file-dialog", target_arch = "wasm32"))]
mod web;

/// Show the system's picker for the file input `target` in `window`, and
/// answer it through `proxy` ([`BlitzShellEvent::FilesChosen`]). False
/// where there is none.
pub(crate) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    let proxy = proxy.clone();
    let window_id = window.id();
    show(
        window,
        request,
        Box::new(move |files| {
            proxy.send_event(BlitzShellEvent::FilesChosen {
                window_id,
                target,
                files,
            })
        }),
    )
}

/// Show the system's picker over `window` for `request`, and wait for the
/// files chosen: `None` for a cancel, or where there is no picker. For a
/// picker a page's own code opens (a menu's Open…, a shortcut), where there
/// is no file input to press; it opens as a file input's would, with the
/// same defaults.
///
/// On the web, a browser opens a picker only during a press: call this from
/// a press's handler there.
pub fn choose_files(window: &Arc<dyn Window>, request: FileRequest) -> ChooseFiles {
    let slot: Slot = Arc::new(Mutex::new((None, None)));
    let answer: Answer = {
        let slot = slot.clone();
        Box::new(move |files| ChooseFiles::fill(&slot, files))
    };
    if !show(window, request, answer) {
        ChooseFiles::fill(&slot, None);
    }
    ChooseFiles(slot)
}

type Slot = Arc<Mutex<(Option<Option<Vec<ChosenFile>>>, Option<std::task::Waker>)>>;

/// The files [`choose_files`] waits for.
pub struct ChooseFiles(Slot);

impl ChooseFiles {
    fn fill(slot: &Slot, files: Option<Vec<ChosenFile>>) {
        let Ok(mut s) = slot.lock() else { return };
        s.0 = Some(files);
        if let Some(w) = s.1.take() {
            w.wake();
        }
    }
}

impl std::future::Future for ChooseFiles {
    type Output = Option<Vec<ChosenFile>>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Ok(mut s) = self.0.lock() else {
            return std::task::Poll::Ready(None);
        };
        match s.0.take() {
            Some(files) => std::task::Poll::Ready(files),
            None => {
                s.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

/// Show the system's picker over `window`, giving its answer to `answer`.
/// False where there is none (and `answer` is dropped, uncalled).
#[allow(unused_variables)]
fn show(window: &Arc<dyn Window>, request: FileRequest, answer: Answer) -> bool {
    #[cfg(all(feature = "file-dialog", target_os = "windows"))]
    return windows::pick(window, request, answer);
    #[cfg(all(feature = "file-dialog", target_os = "macos"))]
    return macos::pick(window, request, answer);
    #[cfg(all(
        feature = "file-dialog",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    return linux::pick(window, request, answer);
    #[cfg(target_os = "ios")]
    return ios::pick(window, request, answer);
    #[cfg(all(feature = "file-dialog", target_arch = "wasm32"))]
    return web::pick(window, request, answer);
    #[allow(unreachable_code)]
    false
}
