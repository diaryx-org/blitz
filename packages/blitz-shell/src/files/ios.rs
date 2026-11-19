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
//! ([`super::answer`]): the delegate's callbacks run on the main thread,
//! where the event loop does.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;

use blitz_traits::files::{Accept, ChosenFile, FileRequest, FileSource};
use blitz_traits::node_id::NodeId;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController};
use objc2_uniform_type_identifiers::{
    UTType, UTTypeAudio, UTTypeFolder, UTTypeImage, UTTypeItem, UTTypeMovie, UTTypeText,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::BlitzShellProxy;

/// What a picker answers to: the window and input that asked.
pub struct Asked {
    proxy: BlitzShellProxy,
    window: Arc<dyn Window>,
    target: NodeId,
    directory: bool,
    /// Answered once: UIKit calls one of the two callbacks, but a picker
    /// dismissed some other way is not left to answer twice.
    answered: Cell<bool>,
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
        let asked = self.ivars();
        if asked.answered.replace(true) {
            return;
        }
        super::answer(&asked.proxy, &*asked.window, asked.target, files);
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

/// The UTTypes `request` offers: a folder for `webkitdirectory`; else
/// those `accept` names, or any document if it names none, or one UIKit
/// does not know (which, left out, could not be offered at all).
fn content_types(request: &FileRequest) -> Vec<Retained<UTType>> {
    let any = || vec![unsafe { UTTypeItem }.retain()];
    if request.directory {
        return vec![unsafe { UTTypeFolder }.retain()];
    }
    if request.accept.is_empty() {
        return any();
    }
    let mut types = Vec::new();
    for accept in &request.accept {
        let ty = match accept {
            Accept::Extension(e) => UTType::typeWithFilenameExtension(&NSString::from_str(e)),
            Accept::Mime(m) => UTType::typeWithMIMEType(&NSString::from_str(m)),
            Accept::Group(g) => match g.as_str() {
                "image" => Some(unsafe { UTTypeImage }.retain()),
                "audio" => Some(unsafe { UTTypeAudio }.retain()),
                "video" => Some(unsafe { UTTypeMovie }.retain()),
                "text" => Some(unsafe { UTTypeText }.retain()),
                _ => None,
            },
        };
        match ty {
            Some(ty) => types.push(ty),
            None => return any(),
        }
    }
    types
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

pub(super) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(presenter) = presenter(&**window) else {
        return false;
    };
    let types = NSArray::from_retained_slice(&content_types(&request));
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
        UIDocumentPickerViewController::alloc(mtm),
        &types,
        !request.directory,
    );
    picker.setAllowsMultipleSelection(request.multiple && !request.directory);
    let delegate = PickerDelegate::new(
        mtm,
        Asked {
            proxy: proxy.clone(),
            window: window.clone(),
            target,
            directory: request.directory,
            answered: Cell::new(false),
        },
    );
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    presenter.presentViewController_animated_completion(&picker, true, None);
    true
}
