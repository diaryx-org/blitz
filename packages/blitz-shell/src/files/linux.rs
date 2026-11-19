//! PATCH(diaryx): Linux's (and the BSDs') picker for a file input: the
//! desktop's own, GNOME's, KDE's or whichever the desktop runs, through the
//! XDG desktop portal's FileChooser
//! (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html>),
//! spoken to over D-Bus by `dbus.rs`. It is the one a sandboxed app (a
//! Flatpak, a Snap) may use at all.
//!
//! - **Types**: `accept`'s extensions are offered as globs, in either case
//!   (GTK's globs are case-sensitive), its MIME types and groups as types.
//! - **Folder**: opens in the last folder used, attached to the window on
//!   X11 (`x11:` and its id); Wayland's needs the window exported through
//!   xdg-foreign, which winit does not do, so the picker is not attached
//!   there.
//! - **No portal** (a bare window manager): `zenity`, else `kdialog`, if
//!   one is installed; else the input hears a cancel.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use blitz_traits::files::{Accept, FileRequest};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::dbus::{Bus, Call, METHOD_RETURN, SIGNAL, Value};

pub(super) fn pick(window: &Arc<dyn Window>, request: FileRequest, answer: super::Answer) -> bool {
    let parent = parent_window(&**window);
    let folder = super::last_folder();
    let directory = request.directory;
    super::on_thread(
        directory,
        move || match portal(&parent, &request, folder.as_ref()) {
            Ok(answer) => answer,
            Err(_) => fallback(&request, folder.as_ref()),
        },
        answer,
    )
}

/// The portal's name for the window the picker belongs to.
fn parent_window(window: &dyn Window) -> String {
    let Ok(handle) = window.window_handle() else {
        return String::new();
    };
    match handle.as_raw() {
        RawWindowHandle::Xlib(h) => format!("x11:{:x}", h.window),
        RawWindowHandle::Xcb(h) => format!("x11:{:x}", h.window.get()),
        _ => String::new(),
    }
}

/// What the picker's title says.
fn title(request: &FileRequest) -> &'static str {
    if request.directory {
        "Choose a Folder"
    } else if request.multiple {
        "Choose Files"
    } else {
        "Choose a File"
    }
}

/// `accept` as the portal's filter: one, named as `request` names it, of
/// globs and types. None when any file will do.
fn filter(request: &FileRequest) -> Option<Value> {
    if request.directory || request.extensions().is_empty() {
        return None;
    }
    let mut rules = Vec::new();
    for a in &request.accept {
        match a {
            Accept::Extension(e) => {
                for glob in globs(e) {
                    rules.push(Value::Struct(vec![Value::U32(0), Value::str(glob)]));
                }
            }
            Accept::Mime(m) => rules.push(Value::Struct(vec![Value::U32(1), Value::str(m)])),
            Accept::Group(g) => rules.push(Value::Struct(vec![
                Value::U32(1),
                Value::str(format!("{g}/*")),
            ])),
        }
    }
    Some(Value::Struct(vec![
        Value::str(request.filter_name()),
        Value::Array("(us)".into(), rules),
    ]))
}

/// An extension's globs: lowercase and uppercase, as files are named both
/// ways (`IMG_0001.JPG`).
fn globs(ext: &str) -> Vec<String> {
    let lower = format!("*.{}", ext.to_lowercase());
    let upper = format!("*.{}", ext.to_uppercase());
    if lower == upper {
        vec![lower]
    } else {
        vec![lower, upper]
    }
}

/// Ask the portal. `Ok(None)` for a cancel; an error where there is no
/// portal to ask.
fn portal(
    parent: &str,
    request: &FileRequest,
    folder: Option<&PathBuf>,
) -> io::Result<Option<Vec<PathBuf>>> {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let mut bus = Bus::session()?;
    let token = format!(
        "blitz{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let sender = bus.unique_name.trim_start_matches(':').replace('.', "_");
    let expected = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    // Listen before asking: the answer may come before the call returns.
    add_match(&mut bus, &expected)?;

    let mut options = vec![
        ("handle_token", Value::str(&token)),
        ("modal", Value::Bool(true)),
    ];
    if request.multiple && !request.directory {
        options.push(("multiple", Value::Bool(true)));
    }
    if request.directory {
        options.push(("directory", Value::Bool(true)));
    }
    if let Some(filter) = filter(request) {
        options.push((
            "filters",
            Value::Array("(sa(us))".into(), vec![filter.clone()]),
        ));
        options.push(("current_filter", filter));
    }
    if let Some(folder) = folder {
        use std::os::unix::ffi::OsStrExt;
        let mut bytes: Vec<Value> = folder
            .as_os_str()
            .as_bytes()
            .iter()
            .map(|b| Value::Byte(*b))
            .collect();
        bytes.push(Value::Byte(0));
        options.push(("current_folder", Value::Array("y".into(), bytes)));
    }
    let reply = bus.call(&Call {
        destination: "org.freedesktop.portal.Desktop",
        path: "/org/freedesktop/portal/desktop",
        interface: "org.freedesktop.portal.FileChooser",
        member: "OpenFile",
        args: vec![
            Value::str(parent),
            Value::str(title(request)),
            Value::dict(options),
        ],
    })?;
    // An older portal names the request its own way.
    let handle = reply
        .body
        .first()
        .and_then(Value::as_str)
        .unwrap_or(&expected)
        .to_string();
    if handle != expected {
        add_match(&mut bus, &handle)?;
    }
    loop {
        let m = bus.receive()?;
        let is_response = m.kind == SIGNAL
            && m.member.as_deref() == Some("Response")
            && m.path
                .as_deref()
                .is_some_and(|p| p == expected || p == handle);
        if !is_response {
            continue;
        }
        // 0 chosen, 1 cancelled, 2 ended some other way.
        if m.body.first().and_then(Value::as_u32) != Some(0) {
            return Ok(None);
        }
        let uris = m
            .body
            .get(1)
            .and_then(|r| r.get("uris"))
            .map(|u| {
                u.items()
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(path_of)
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Some(uris));
    }
}

fn add_match(bus: &mut Bus, path: &str) -> io::Result<()> {
    let rule = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{path}'"
    );
    let m = bus.call(&Call {
        destination: "org.freedesktop.DBus",
        path: "/org/freedesktop/DBus",
        interface: "org.freedesktop.DBus",
        member: "AddMatch",
        args: vec![Value::str(rule)],
    })?;
    debug_assert_eq!(m.kind, METHOD_RETURN);
    Ok(())
}

/// A `file://` URI's path, its escapes undone.
fn path_of(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    // `file://host/path`: only this machine's.
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) if &rest[..i] == "localhost" => &rest[i..],
        _ => return None,
    };
    let out = super::dbus::percent_decode(path);
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// With no portal: zenity, else kdialog. A cancel, or neither installed,
/// is `None`.
fn fallback(request: &FileRequest, folder: Option<&PathBuf>) -> Option<Vec<PathBuf>> {
    zenity(request, folder)
        .or_else(|| kdialog(request, folder))
        .flatten()
}

/// The paths a dialog printed, one a line; `None` (inside) for a cancel;
/// `None` (outside) where it could not be run.
fn run(cmd: &mut Command) -> Option<Option<Vec<PathBuf>>> {
    let out = cmd.output().ok()?;
    if !out.status.success() {
        // Exit 1: cancelled.
        return Some(None);
    }
    let paths: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    Some((!paths.is_empty()).then_some(paths))
}

fn zenity(request: &FileRequest, folder: Option<&PathBuf>) -> Option<Option<Vec<PathBuf>>> {
    let mut cmd = Command::new("zenity");
    cmd.args([
        "--file-selection",
        "--title",
        title(request),
        "--separator=\n",
    ]);
    if request.directory {
        cmd.arg("--directory");
    } else if request.multiple {
        cmd.arg("--multiple");
    }
    // A name in the folder, not the folder: given the folder itself,
    // zenity 4 (GTK 4) opens its parent with the folder selected.
    if let Some(f) = folder {
        cmd.arg(format!("--filename={}", f.join("x").display()));
    }
    let exts = request.extensions();
    if !exts.is_empty() && !request.directory {
        let globs: Vec<String> = exts.iter().flat_map(|e| globs(e)).collect();
        cmd.arg(format!(
            "--file-filter={} | {}",
            request.filter_name(),
            globs.join(" ")
        ));
    }
    run(&mut cmd)
}

fn kdialog(request: &FileRequest, folder: Option<&PathBuf>) -> Option<Option<Vec<PathBuf>>> {
    let mut cmd = Command::new("kdialog");
    cmd.args(["--title", title(request)]);
    let start = folder
        .map(|f| f.display().to_string())
        .unwrap_or_else(|| ".".into());
    if request.directory {
        cmd.args(["--getexistingdirectory", &start]);
    } else {
        cmd.args(["--getopenfilename", &start]);
        let exts = request.extensions();
        if !exts.is_empty() {
            let globs: Vec<String> = exts.iter().flat_map(|e| globs(e)).collect();
            cmd.arg(format!("{}|{}", globs.join(" "), request.filter_name()));
        }
        if request.multiple {
            cmd.args(["--multiple", "--separate-output"]);
        }
    }
    run(&mut cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real portal, over a real session bus: run by hand, with a
    /// desktop for the portal to show its picker on, someone (or xdotool) to
    /// choose `$BLITZ_PICK_EXPECT` in it, and `--ignored`.
    #[test]
    #[ignore]
    fn the_portal_answers() {
        let expect = std::env::var("BLITZ_PICK_EXPECT").expect("BLITZ_PICK_EXPECT");
        let request = FileRequest::from_attributes(Some(".kdbx"), false, false);
        let folder = PathBuf::from(&expect).parent().map(PathBuf::from);
        let answer = portal("", &request, folder.as_ref()).expect("a portal to ask");
        assert_eq!(answer, Some(vec![PathBuf::from(expect)]));
    }

    /// zenity, where there is no portal: by hand, as the portal's test.
    #[test]
    #[ignore]
    fn zenity_answers() {
        let expect = std::env::var("BLITZ_PICK_EXPECT").expect("BLITZ_PICK_EXPECT");
        let request = FileRequest::from_attributes(Some(".kdbx"), false, false);
        let folder = PathBuf::from(&expect).parent().map(PathBuf::from);
        let answer = fallback(&request, folder.as_ref());
        assert_eq!(answer, Some(vec![PathBuf::from(expect)]));
    }

    #[test]
    fn a_uri_is_a_path() {
        assert_eq!(
            path_of("file:///home/a%20b/x.md"),
            Some(PathBuf::from("/home/a b/x.md"))
        );
        assert_eq!(
            path_of("file://localhost/tmp/y"),
            Some(PathBuf::from("/tmp/y"))
        );
        assert_eq!(path_of("file://elsewhere/tmp/y"), None);
        assert_eq!(path_of("https://example.com/"), None);
    }

    #[test]
    fn accept_is_the_portals_filter() {
        let r = FileRequest::from_attributes(Some(".kdbx,image/*"), false, false);
        let Some(Value::Struct(f)) = filter(&r) else {
            panic!()
        };
        assert_eq!(
            f[0].as_str(),
            Some(
                ".kdbx, .png, .jpg, .jpeg, .gif, .webp, .avif, .heic, .heif, .svg, .bmp, .ico, .tif, .tiff"
            )
        );
        let rules: Vec<String> = f[1]
            .items()
            .iter()
            .map(|r| match r {
                Value::Struct(v) => {
                    format!("{}:{}", v[0].as_u32().unwrap(), v[1].as_str().unwrap())
                }
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(rules, ["0:*.kdbx", "0:*.KDBX", "1:image/*"]);
        assert!(filter(&FileRequest::default()).is_none());
    }
}
