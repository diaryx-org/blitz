//! PATCH(diaryx): the Mac's picker for a file input: AppKit's open panel
//! (`NSOpenPanel`), as a sheet on the window that asked, as Safari's is.
//!
//! - **Types**: `accept` as UTTypes (`uttypes.rs`), so the panel offers
//!   only what the input takes; none when any file will do.
//! - **Folder**: the last one a file was chosen from; for `webkitdirectory`,
//!   folders alone may be chosen.
//! - The panel runs on the main thread, where AppKit wants it; its
//!   completion hands the paths to a thread of their own, where a folder's
//!   files are listed, and the answer goes back as every picker's does.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use blitz_traits::files::FileRequest;
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSView, NSWindow};
use objc2_foundation::{NSArray, NSString, NSURL};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// The AppKit window `window` is drawn in.
fn ns_window(window: &dyn Window) -> Option<Retained<NSWindow>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit's AppKit window gives its own content view here, alive
    // as long as the window is.
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

pub(super) fn pick(window: &Arc<dyn Window>, request: FileRequest, answer: super::Answer) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(!request.directory);
    panel.setCanChooseDirectories(request.directory);
    panel.setAllowsMultipleSelection(request.multiple && !request.directory);
    panel.setCanCreateDirectories(false);
    panel.setResolvesAliases(true);
    if let Some(types) = super::uttypes::of(&request).filter(|_| !request.directory) {
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
    }
    if let Some(folder) = super::last_folder() {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&folder.to_string_lossy()));
        panel.setDirectoryURL(Some(&url));
    }

    let parent = ns_window(&**window);
    let directory = request.directory;
    let chosen = panel.clone();
    // Answered once: the handler is called once, and a second call (which
    // AppKit does not make) finds the answer taken.
    let answer = Cell::new(Some(answer));
    let done = RcBlock::new(move |response: NSModalResponse| {
        let Some(answer) = answer.take() else {
            return;
        };
        let paths: Option<Vec<PathBuf>> = (response == NSModalResponseOK).then(|| {
            chosen
                .URLs()
                .iter()
                .filter_map(|u| u.path().map(|p| PathBuf::from(p.to_string())))
                .collect()
        });
        super::on_thread(directory, move || paths, answer);
    });
    match parent {
        // A sheet on the window that asked, as a browser's is.
        Some(parent) => panel.beginSheetModalForWindow_completionHandler(&parent, &done),
        None => panel.beginWithCompletionHandler(&done),
    }
    true
}
