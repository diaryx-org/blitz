//! PATCH(diaryx): Windows' picker for a file input: the Common Item
//! Dialog (`IFileOpenDialog`), the window's child, as Edge's is.
//!
//! Its COM interfaces are declared here by hand, as the Windows SDK's
//! `shobjidl_core.h` lays out their tables of methods, on top of
//! windows-sys's functions, types and constants (which winit already
//! builds): no bindings crate. Only the methods called are typed; the
//! others keep their places in the tables as untyped pointers.
//!
//! The dialog runs on a thread of its own, in a single-threaded apartment
//! of its own, owned by the window that asked (which it disables while it is
//! up, as a modal dialog does); the window's own thread keeps drawing.
//!
//! - **Types**: `accept`'s extensions as one filter (`*.png;*.jpg`, which
//!   Windows matches in any case); none when any file will do.
//! - **Folder**: the last one a file was chosen from; `webkitdirectory`
//!   picks folders.

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;

use blitz_traits::files::FileRequest;
use blitz_traits::node_id::NodeId;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows_sys::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::BlitzShellProxy;

const IID_IFILEOPENDIALOG: GUID = GUID::from_u128(0xd57c7288_d4ad_4768_be02_9d969532d960);
const IID_ISHELLITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);

/// A method these tables keep the place of, never called here.
type Unused = *const c_void;

#[repr(C)]
struct IUnknownVtbl {
    query_interface: Unused,
    add_ref: Unused,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

/// IModalWindow, then IFileDialog, then IFileOpenDialog.
#[repr(C)]
struct IFileOpenDialogVtbl {
    unknown: IUnknownVtbl,
    // IModalWindow
    show: unsafe extern "system" fn(this: *mut c_void, owner: HWND) -> HRESULT,
    // IFileDialog
    set_file_types: unsafe extern "system" fn(
        this: *mut c_void,
        count: u32,
        specs: *const COMDLG_FILTERSPEC,
    ) -> HRESULT,
    set_file_type_index: unsafe extern "system" fn(this: *mut c_void, index: u32) -> HRESULT,
    get_file_type_index: Unused,
    advise: Unused,
    unadvise: Unused,
    set_options: unsafe extern "system" fn(this: *mut c_void, options: u32) -> HRESULT,
    get_options: unsafe extern "system" fn(this: *mut c_void, options: *mut u32) -> HRESULT,
    set_default_folder: Unused,
    set_folder: unsafe extern "system" fn(this: *mut c_void, item: *mut c_void) -> HRESULT,
    get_folder: Unused,
    get_current_selection: Unused,
    set_file_name: Unused,
    get_file_name: Unused,
    set_title: unsafe extern "system" fn(this: *mut c_void, title: PCWSTR) -> HRESULT,
    set_ok_button_label: Unused,
    set_file_name_label: Unused,
    get_result: unsafe extern "system" fn(this: *mut c_void, item: *mut *mut c_void) -> HRESULT,
    add_place: Unused,
    set_default_extension: Unused,
    close: Unused,
    set_client_guid: Unused,
    clear_client_data: Unused,
    set_filter: Unused,
    // IFileOpenDialog
    get_results: unsafe extern "system" fn(this: *mut c_void, items: *mut *mut c_void) -> HRESULT,
    get_selected_items: Unused,
}

#[repr(C)]
struct IShellItemVtbl {
    unknown: IUnknownVtbl,
    bind_to_handler: Unused,
    get_parent: Unused,
    get_display_name:
        unsafe extern "system" fn(this: *mut c_void, kind: i32, name: *mut PWSTR) -> HRESULT,
    get_attributes: Unused,
    compare: Unused,
}

#[repr(C)]
struct IShellItemArrayVtbl {
    unknown: IUnknownVtbl,
    bind_to_handler: Unused,
    get_property_store: Unused,
    get_property_description_list: Unused,
    get_attributes: Unused,
    get_count: unsafe extern "system" fn(this: *mut c_void, count: *mut u32) -> HRESULT,
    get_item_at:
        unsafe extern "system" fn(this: *mut c_void, index: u32, item: *mut *mut c_void) -> HRESULT,
    enum_items: Unused,
}

/// A COM object: a pointer to its table of methods, and its reference,
/// released when it goes.
struct Com<V>(*mut c_void, std::marker::PhantomData<V>);

impl<V> Com<V> {
    /// SAFETY: `ptr` is an object whose table is a `V`, of which we own one
    /// reference.
    unsafe fn from_raw(ptr: *mut c_void) -> Option<Self> {
        (!ptr.is_null()).then_some(Com(ptr, std::marker::PhantomData))
    }

    fn vtbl(&self) -> &V {
        // SAFETY: a COM object starts with the pointer to its table.
        unsafe { &**(self.0 as *const *const V) }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: every table starts with IUnknown's.
        unsafe {
            let unknown = &**(self.0 as *const *const IUnknownVtbl);
            (unknown.release)(self.0);
        }
    }
}

fn succeeded(hr: HRESULT) -> bool {
    hr >= 0
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// A shell item's path on disk.
fn path_of(item: &Com<IShellItemVtbl>) -> Option<PathBuf> {
    let mut name: PWSTR = null_mut();
    // SAFETY: GetDisplayName gives a string we own and free.
    unsafe {
        if !succeeded((item.vtbl().get_display_name)(
            item.0,
            SIGDN_FILESYSPATH,
            &mut name,
        )) || name.is_null()
        {
            return None;
        }
        let len = (0..).take_while(|&i| *name.add(i) != 0).count();
        let path = OsString::from_wide(std::slice::from_raw_parts(name, len));
        CoTaskMemFree(name as *const c_void);
        Some(PathBuf::from(path))
    }
}

/// The window's handle, to own the dialog.
fn owner(window: &dyn Window) -> HWND {
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as HWND,
        _ => null_mut(),
    }
}

/// What the dialog's title says.
fn title(request: &FileRequest) -> &'static str {
    if request.directory {
        "Choose a Folder"
    } else if request.multiple {
        "Choose Files"
    } else {
        "Choose a File"
    }
}

/// Show the dialog, on this thread, and wait for it: the paths chosen, or
/// `None` for a cancel or a dialog that could not be shown.
fn show(owner: HWND, request: &FileRequest, folder: Option<&Path>) -> Option<Vec<PathBuf>> {
    // SAFETY: COM, as its documentation has it: an apartment of this
    // thread's own, objects released before it is left.
    unsafe {
        let init = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let answer = (|| {
            let mut raw = null_mut();
            let hr = CoCreateInstance(
                &FileOpenDialog,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IFILEOPENDIALOG,
                &mut raw,
            );
            if !succeeded(hr) {
                return None;
            }
            let dialog = Com::<IFileOpenDialogVtbl>::from_raw(raw)?;
            let v = dialog.vtbl();

            let mut options = 0u32;
            (v.get_options)(dialog.0, &mut options);
            options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
            if request.directory {
                options |= FOS_PICKFOLDERS;
            } else {
                options |= FOS_FILEMUSTEXIST;
                if request.multiple {
                    options |= FOS_ALLOWMULTISELECT;
                }
            }
            (v.set_options)(dialog.0, options);
            let title = wide(title(request));
            (v.set_title)(dialog.0, title.as_ptr());

            // `accept` as one filter. The strings live until the dialog is
            // shown and gone.
            let extensions = if request.directory {
                Vec::new()
            } else {
                request.extensions()
            };
            let name = wide(&request.filter_name());
            let spec = wide(
                &extensions
                    .iter()
                    .map(|e| format!("*.{e}"))
                    .collect::<Vec<_>>()
                    .join(";"),
            );
            if !extensions.is_empty() {
                let filter = COMDLG_FILTERSPEC {
                    pszName: name.as_ptr(),
                    pszSpec: spec.as_ptr(),
                };
                (v.set_file_types)(dialog.0, 1, &filter);
                (v.set_file_type_index)(dialog.0, 1);
            }

            if let Some(folder) = folder {
                let path = wide_path(folder);
                let mut item = null_mut();
                if succeeded(SHCreateItemFromParsingName(
                    path.as_ptr(),
                    null_mut(),
                    &IID_ISHELLITEM,
                    &mut item,
                )) && let Some(item) = Com::<IShellItemVtbl>::from_raw(item)
                {
                    (v.set_folder)(dialog.0, item.0);
                }
            }

            // Cancelled: ERROR_CANCELLED, as an HRESULT.
            if !succeeded((v.show)(dialog.0, owner)) {
                return None;
            }

            let mut paths = Vec::new();
            if request.multiple && !request.directory {
                let mut raw = null_mut();
                if !succeeded((v.get_results)(dialog.0, &mut raw)) {
                    return None;
                }
                let items = Com::<IShellItemArrayVtbl>::from_raw(raw)?;
                let mut count = 0u32;
                (items.vtbl().get_count)(items.0, &mut count);
                for i in 0..count {
                    let mut raw = null_mut();
                    if succeeded((items.vtbl().get_item_at)(items.0, i, &mut raw))
                        && let Some(item) = Com::<IShellItemVtbl>::from_raw(raw)
                    {
                        paths.extend(path_of(&item));
                    }
                }
            } else {
                let mut raw = null_mut();
                if !succeeded((v.get_result)(dialog.0, &mut raw)) {
                    return None;
                }
                let item = Com::<IShellItemVtbl>::from_raw(raw)?;
                paths.extend(path_of(&item));
            }
            Some(paths)
        })();
        if succeeded(init) {
            CoUninitialize();
        }
        answer
    }
}

pub(super) fn pick(
    window: &Arc<dyn Window>,
    proxy: &BlitzShellProxy,
    target: NodeId,
    request: FileRequest,
) -> bool {
    // An HWND is a number; it crosses to the dialog's thread as one.
    let owner = owner(&**window) as isize;
    let folder = super::last_folder();
    let directory = request.directory;
    super::on_thread(window, proxy, target, directory, move || {
        show(owner as HWND, &request, folder.as_deref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real dialog: run by hand under Windows (or Wine), with someone
    /// (or xdotool) to choose `$BLITZ_PICK_EXPECT` in it, and `--ignored`.
    #[test]
    #[ignore]
    fn the_dialog_answers() {
        let expect = PathBuf::from(std::env::var("BLITZ_PICK_EXPECT").expect("BLITZ_PICK_EXPECT"));
        let request = FileRequest::from_attributes(Some(".kdbx"), false, false);
        let answer = show(null_mut(), &request, expect.parent());
        assert_eq!(answer, Some(vec![expect]));
    }
}
