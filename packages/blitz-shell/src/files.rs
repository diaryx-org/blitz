//! PATCH(diaryx): the system's file picker, for `<input type=file>`.
//!
//! [`pick`] shows it for a file input and returns at once; the picker's
//! answer reaches the window as [`BlitzShellEvent::FilesChosen`], which the
//! application hands to the document. The window keeps drawing while the
//! picker is open, as a browser's does.
//!
//! - **macOS, Windows, Linux and the BSDs**: rfd's dialog, a sheet on the
//!   window on the Mac and its child elsewhere, filtered by the input's
//!   `accept`, opening in the folder the last file was chosen from. On
//!   Linux, the desktop's own through its portal.
//!
//! Elsewhere there is no picker, and the document falls back to
//! `ShellProvider::open_file_dialog`.

use std::sync::Arc;

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

#[cfg(all(
    feature = "file-dialog",
    any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
mod desktop {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    use blitz_traits::files::{ChosenFile, FileRequest};
    use blitz_traits::node_id::NodeId;
    use winit::window::Window;

    use crate::BlitzShellProxy;

    /// The folder the last file was chosen from, which the next picker
    /// opens in, as a browser's does.
    static LAST_FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

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
        if let Some(folder) = LAST_FOLDER.lock().ok().and_then(|f| f.clone()) {
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
        let proxy = proxy.clone();
        let window = window.clone();
        let spawned = std::thread::Builder::new()
            .name("blitz-file-picker".into())
            .spawn(move || {
                let picked = block_on(picking);
                let files = picked.map(|handles| {
                    let paths: Vec<PathBuf> =
                        handles.iter().map(|h| h.path().to_path_buf()).collect();
                    let folder = if request.directory {
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
                            if request.directory {
                                ChosenFile::from_folder(&p)
                            } else {
                                ChosenFile::from_path(p).into_iter().collect()
                            }
                        })
                        .collect::<Vec<_>>()
                });
                super::answer(&proxy, &*window, target, files);
            });
        spawned.is_ok()
    }

    /// Wait for `future` on this thread: a picker's, whose own backend does
    /// the waiting (a sheet's completion on the Mac, a portal's response on
    /// Linux, a thread of its own on Windows).
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

/// Show the system's picker for the file input `target` in `window`, and
/// answer through `proxy`. False where there is none.
#[allow(unused_variables)]
pub(crate) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    #[cfg(all(
        feature = "file-dialog",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    return desktop::pick(window, proxy, target, request);
    #[allow(unreachable_code)]
    false
}
