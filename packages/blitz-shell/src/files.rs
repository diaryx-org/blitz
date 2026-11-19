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
//! - **Windows**: rfd's dialog, the window's child.
//! - **Linux and the BSDs** (`linux.rs`): the desktop's own picker, through
//!   the XDG desktop portal over D-Bus (`dbus.rs`, a client of our own), or
//!   zenity or kdialog where there is no portal.
//! - **iOS** (`ios.rs`): UIKit's document picker over the window.
//!
//! Elsewhere there is no picker, and the document falls back to
//! `ShellProvider::open_file_dialog`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use blitz_traits::files::{ChosenFile, FileRequest};
use blitz_traits::node_id::NodeId;
use winit::window::Window;

use crate::{BlitzShellEvent, BlitzShellProxy};

/// The answer for `target`, to the window that asked.
fn answer(
    proxy: &BlitzShellProxy,
    window: &dyn Window,
    target: NodeId,
    files: Option<Vec<ChosenFile>>,
) {
    proxy.send_event(BlitzShellEvent::FilesChosen {
        window_id: window.id(),
        target,
        files,
    });
}

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
/// as long as the person takes, and answer `target` with what it gives:
/// `None` for a cancel (or a picker that failed).
#[allow(dead_code)]
fn on_thread(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    directory: bool,
    pick: impl FnOnce() -> Option<Vec<PathBuf>> + Send + 'static,
) -> bool {
    let proxy = proxy.clone();
    let window = window.clone();
    std::thread::Builder::new()
        .name("blitz-file-picker".into())
        .spawn(move || {
            let files = pick()
                .map(|paths| chosen(paths, directory))
                .filter(|f| !f.is_empty());
            answer(&proxy, &*window, target, files);
        })
        .is_ok()
}

#[cfg(all(feature = "file-dialog", target_os = "windows"))]
mod desktop {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    use blitz_traits::files::FileRequest;
    use blitz_traits::node_id::NodeId;
    use winit::window::Window;

    use crate::BlitzShellProxy;

    type Picked = Option<Vec<rfd::FileHandle>>;

    pub(super) fn pick(
        window: &Arc<dyn Window>,
        proxy: &BlitzShellProxy,
        target: NodeId,
        request: FileRequest,
    ) -> bool {
        let mut dialog = rfd::AsyncFileDialog::new().set_parent(&**window);
        let extensions = request.extensions();
        if !extensions.is_empty() && !request.directory {
            dialog = dialog.add_filter(request.filter_name(), &extensions);
        }
        if let Some(folder) = super::last_folder() {
            dialog = dialog.set_directory(folder);
        }
        let picking: Pin<Box<dyn Future<Output = Picked> + Send>> = if request.directory {
            let f = dialog.pick_folder();
            Box::pin(async move { f.await.map(|h| vec![h]) })
        } else if request.multiple {
            Box::pin(dialog.pick_files())
        } else {
            let f = dialog.pick_file();
            Box::pin(async move { f.await.map(|h| vec![h]) })
        };
        super::on_thread(window, proxy, target, request.directory, move || {
            block_on(picking).map(|handles| {
                handles
                    .iter()
                    .map(|h| h.path().to_path_buf())
                    .collect::<Vec<PathBuf>>()
            })
        })
    }

    /// Wait for `future` on this thread: a picker's, whose own backend does
    /// the waiting (a thread of its own, on Windows).
    fn block_on<T>(future: impl Future<Output = T>) -> T {
        struct Unpark(std::thread::Thread);
        impl Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::park();
        }
    }
}

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

/// Show the system's picker for the file input `target` in `window`, and
/// answer through `proxy`. False where there is none.
#[allow(unused_variables)]
pub(crate) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    #[cfg(all(feature = "file-dialog", target_os = "windows"))]
    return desktop::pick(window, proxy, target, request);
    #[cfg(all(feature = "file-dialog", target_os = "macos"))]
    return macos::pick(window, proxy, target, request);
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
    return linux::pick(window, proxy, target, request);
    #[cfg(target_os = "ios")]
    return ios::pick(window, proxy, target, request);
    #[allow(unreachable_code)]
    false
}
