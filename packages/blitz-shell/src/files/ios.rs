//! PATCH(diaryx): iOS's picker for a file input: UIKit's document picker
//! (`UIDocumentPickerViewController`), presented over the window's view
//! controller, as Safari presents it for `<input type=file>`.
//!
//! - **Types**: `accept`'s extensions and MIME types become UTTypes, and
//!   `image/*`, `audio/*`, `video/*` and `text/*` UIKit's own groups; a
//!   type UIKit does not know, or no `accept`, offers any document.
//! - **Files** are taken as copies (`asCopy`): UIKit puts each in the app's
//!   own temporary folder, readable there by path like any file, so the
//!   page reads them when it likes.
//! - **A folder** (`webkitdirectory`) cannot be copied: it is opened where
//!   it is, and its files are read while UIKit grants access to it, then
//!   handed over whole.
//!
//! The answer goes back to the window as every picker's does
//! (`files.rs`'s `Answer`): the delegate's callbacks run on the main thread,
//! where the event loop does.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;

use blitz_traits::files::{ChosenFile, FileRequest, FileSource};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// What a picker answers to.
pub struct Asked {
    directory: bool,
    /// Answered once: UIKit calls one of the two callbacks, but a picker
    /// dismissed some other way is not left to answer twice.
    answer: Cell<Option<super::Answer>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzDocumentPickerDelegate"]
    #[ivars = Asked]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let urls: Vec<Retained<NSURL>> = urls.iter().collect();
            let files = chosen(&urls, self.ivars().directory);
            self.answer((!files.is_empty()).then_some(files));
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.answer(None);
        }
    }
);

impl PickerDelegate {
    fn new(mtm: MainThreadMarker, asked: Asked) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(asked);
        unsafe { msg_send![super(this), init] }
    }

    fn answer(&self, files: Option<Vec<ChosenFile>>) {
        if let Some(answer) = self.ivars().answer.take() {
            answer(files);
        }
    }
}

thread_local! {
    /// The delegate of the picker on screen: the picker holds its delegate
    /// weakly, so it is kept here until the next picker replaces it (never
    /// dropped from inside its own callback).
    static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
}

/// The files a picker's URLs are.
fn chosen(urls: &[Retained<NSURL>], directory: bool) -> Vec<ChosenFile> {
    let mut out = Vec::new();
    for url in urls {
        let Some(path) = url.path().map(|p| PathBuf::from(p.to_string())) else {
            continue;
        };
        if !directory {
            // A copy in the app's own temporary folder: readable as it is.
            out.extend(ChosenFile::from_path(path));
            continue;
        }
        // A folder where it is: readable only while access to it is granted,
        // so its files are read now and handed over whole.
        let granted = unsafe { url.startAccessingSecurityScopedResource() };
        for file in ChosenFile::from_folder(&path) {
            if let Ok(bytes) = file.read() {
                out.push(ChosenFile {
                    source: FileSource::Bytes(bytes),
                    ..file
                });
            }
        }
        if granted {
            unsafe { url.stopAccessingSecurityScopedResource() };
        }
    }
    out
}

/// The view controller to present over: the window's, or what it already
/// presents (a sheet the page put up), topmost.
fn presenter(window: &dyn Window) -> Option<Retained<UIViewController>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return None;
    };
    let controller = handle.ui_view_controller?;
    // SAFETY: winit's UIKit window gives its own view controller here, alive
    // as long as the window is.
    let mut top: Retained<UIViewController> =
        unsafe { controller.cast::<UIViewController>().as_ref() }.retain();
    while let Some(presented) = top.presentedViewController() {
        top = presented;
    }
    Some(top)
}

pub(super) fn pick(window: &Arc<dyn Window>, request: FileRequest, answer: super::Answer) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(presenter) = presenter(&**window) else {
        return false;
    };
    let types = super::uttypes::of(&request).unwrap_or_else(super::uttypes::any);
    let types = NSArray::from_retained_slice(&types);
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
        UIDocumentPickerViewController::alloc(mtm),
        &types,
        !request.directory,
    );
    picker.setAllowsMultipleSelection(request.multiple && !request.directory);
    let delegate = PickerDelegate::new(
        mtm,
        Asked {
            directory: request.directory,
            answer: Cell::new(Some(answer)),
        },
    );
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    presenter.presentViewController_animated_completion(&picker, true, None);
    true
}
