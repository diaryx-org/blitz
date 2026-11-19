//! PATCH(diaryx): a file input's `accept` as Apple's UTTypes, which the
//! Mac's open panel (`macos.rs`) and iOS's document picker (`ios.rs`) both
//! filter by.

use blitz_traits::files::{Accept, FileRequest};
use objc2::Message;
use objc2::rc::Retained;
use objc2_foundation::NSString;
use objc2_uniform_type_identifiers::{
    UTType, UTTypeAudio, UTTypeFolder, UTTypeImage, UTTypeItem, UTTypeMovie, UTTypeText,
};

/// The types `request` offers: a folder, for `webkitdirectory`; else those
/// `accept` names. `None` when any file will do: no `accept`, or one
/// naming a type the system does not know (which, left out, could not be
/// offered at all).
pub(super) fn of(request: &FileRequest) -> Option<Vec<Retained<UTType>>> {
    if request.directory {
        return Some(vec![unsafe { UTTypeFolder }.retain()]);
    }
    if request.accept.is_empty() {
        return None;
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
        types.push(ty?);
    }
    Some(types)
}

/// Any document: what a picker that must be given types offers when any
/// file will do.
#[allow(dead_code)]
pub(super) fn any() -> Vec<Retained<UTType>> {
    vec![unsafe { UTTypeItem }.retain()]
}
