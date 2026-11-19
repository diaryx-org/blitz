//! PATCH(diaryx): what an `<input type=file>` asks the system's picker for
//! ([`FileRequest`]), and what it is given back ([`ChosenFile`]).
//!
//! A shell answers [`ShellProvider::pick_files`](crate::shell::ShellProvider::pick_files)
//! later, with [`UiEvent::FilesChosen`](crate::events::UiEvent::FilesChosen):
//! the picker never holds up the window that asked for it.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use bytes::Bytes;

/// One of the comma-separated tokens of an input's `accept`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Accept {
    /// `.png`: an extension, lowercase, without its dot.
    Extension(String),
    /// `image/png`: a MIME type, lowercase.
    Mime(String),
    /// `image/*`: every type of a MIME group (`image`, `audio`, `video`,
    /// `text`), lowercase.
    Group(String),
}

impl Accept {
    /// An input's `accept`, as the HTML standard reads it: tokens split on
    /// commas, trimmed, lowercased; empty or unreadable ones dropped.
    pub fn parse(accept: &str) -> Vec<Accept> {
        accept
            .split(',')
            .map(|t| t.trim().to_ascii_lowercase())
            .filter_map(|t| {
                if let Some(ext) = t.strip_prefix('.') {
                    (!ext.is_empty() && !ext.contains(['.', '/']))
                        .then(|| Accept::Extension(ext.into()))
                } else if let Some(group) = t.strip_suffix("/*") {
                    (!group.is_empty() && !group.contains('/')).then(|| Accept::Group(group.into()))
                } else if t.split('/').count() == 2 && !t.starts_with('/') && !t.ends_with('/') {
                    Some(Accept::Mime(t))
                } else {
                    None
                }
            })
            .collect()
    }

    /// The extensions this token stands for: the extension itself, or those
    /// [`TYPES`] gives its MIME type or group. None for a type it does not
    /// know.
    pub fn extensions(&self) -> Vec<&str> {
        match self {
            Accept::Extension(e) => vec![e.as_str()],
            Accept::Mime(m) => TYPES
                .iter()
                .filter(|(_, mime)| mime == m)
                .map(|(ext, _)| *ext)
                .collect(),
            Accept::Group(g) => TYPES
                .iter()
                .filter(|(_, mime)| mime.split('/').next() == Some(g.as_str()))
                .map(|(ext, _)| *ext)
                .collect(),
        }
    }

    /// Whether a file of `name` (and of MIME type `mime`, if known) is one
    /// this token accepts.
    pub fn accepts(&self, name: &str, mime: Option<&str>) -> bool {
        let ext = extension(name);
        match self {
            Accept::Extension(e) => ext.as_deref() == Some(e.as_str()),
            Accept::Mime(m) => {
                mime == Some(m.as_str())
                    || ext.is_some_and(|x| self.extensions().contains(&x.as_str()))
            }
            Accept::Group(g) => {
                mime.is_some_and(|m| m.split('/').next() == Some(g.as_str()))
                    || ext.is_some_and(|x| self.extensions().contains(&x.as_str()))
            }
        }
    }
}

/// Extensions and the MIME types they are, for turning `accept`'s types
/// into the extensions a system's picker filters by, and a file's name into
/// its type. The common types a page asks for; anything else is offered by
/// its extension, or as any file.
pub const TYPES: &[(&str, &str)] = &[
    // Images
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("svg", "image/svg+xml"),
    ("bmp", "image/bmp"),
    ("ico", "image/vnd.microsoft.icon"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    // Audio
    ("mp3", "audio/mpeg"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("wav", "audio/wav"),
    ("flac", "audio/flac"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("opus", "audio/opus"),
    ("weba", "audio/webm"),
    // Video
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("mov", "video/quicktime"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
    ("ogv", "video/ogg"),
    ("avi", "video/x-msvideo"),
    // Text
    ("txt", "text/plain"),
    ("text", "text/plain"),
    ("md", "text/markdown"),
    ("markdown", "text/markdown"),
    ("csv", "text/csv"),
    ("tsv", "text/tab-separated-values"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("ics", "text/calendar"),
    ("vcf", "text/vcard"),
    // Documents and data
    ("pdf", "application/pdf"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("toml", "application/toml"),
    ("epub", "application/epub+zip"),
    ("rtf", "application/rtf"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("odp", "application/vnd.oasis.opendocument.presentation"),
    // Archives
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("tar", "application/x-tar"),
    ("7z", "application/x-7z-compressed"),
    // Fonts
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
];

/// A file name's extension, lowercase.
fn extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// The MIME type a file's name says it is, if [`TYPES`] knows it.
pub fn content_type_of(name: &str) -> Option<&'static str> {
    let ext = extension(name)?;
    TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// What a file input asks the system's picker for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileRequest {
    /// `multiple`: more than one file may be chosen.
    pub multiple: bool,
    /// `webkitdirectory`: a folder is chosen, and its files are the input's.
    pub directory: bool,
    /// `accept`, parsed. Empty: any file.
    pub accept: Vec<Accept>,
}

impl FileRequest {
    /// The request an input's attributes make.
    pub fn from_attributes(accept: Option<&str>, multiple: bool, directory: bool) -> Self {
        FileRequest {
            multiple,
            directory,
            accept: accept.map(Accept::parse).unwrap_or_default(),
        }
    }

    /// The extensions to filter the picker by, each once, in the order
    /// `accept` gives them. Empty when any file will do: no `accept`, or
    /// one only of types no extension is known for (which a picker filtering
    /// by extension could then not offer at all).
    pub fn extensions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in &self.accept {
            let exts = a.extensions();
            if exts.is_empty() {
                // A type with no known extension: rather than hide every file
                // of it, offer any file.
                return Vec::new();
            }
            for e in exts {
                if !out.iter().any(|o| o == e) {
                    out.push(e.to_string());
                }
            }
        }
        out
    }

    /// A name for the filter, as a picker shows it: "Images" for `image/*`,
    /// else the extensions it offers.
    pub fn filter_name(&self) -> String {
        if let [Accept::Group(g)] = self.accept.as_slice() {
            return match g.as_str() {
                "image" => "Images".into(),
                "audio" => "Audio".into(),
                "video" => "Videos".into(),
                "text" => "Text".into(),
                "font" => "Fonts".into(),
                other => format!("{other} files"),
            };
        }
        let exts = self.extensions();
        if exts.is_empty() {
            "All files".into()
        } else {
            exts.iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// Whether a chosen file is one `accept` allows: a picker that cannot
    /// filter by type is checked after it.
    pub fn accepts(&self, file: &ChosenFile) -> bool {
        self.accept.is_empty()
            || self.extensions().is_empty()
            || self
                .accept
                .iter()
                .any(|a| a.accepts(&file.name, file.content_type.as_deref()))
    }
}

/// Where a chosen file's contents are.
#[derive(Clone, Debug)]
pub enum FileSource {
    /// A file on disk, read when the page asks for it.
    Path(PathBuf),
    /// Contents the system handed over: a browser's `File`, read, or a
    /// document a phone's picker shared.
    Bytes(Bytes),
}

/// A file the person chose.
#[derive(Clone, Debug)]
pub struct ChosenFile {
    /// Its name, without a folder.
    pub name: String,
    /// Within the folder chosen, for an input that chooses a folder
    /// (`webkitdirectory`): `Photos/2026/a.jpg`. Else its name.
    pub relative_path: String,
    pub size: u64,
    /// When it was last changed, in milliseconds since 1970.
    pub last_modified: u64,
    /// Its MIME type, where known.
    pub content_type: Option<String>,
    pub source: FileSource,
}

impl ChosenFile {
    /// A file on disk, described from its metadata.
    pub fn from_path(path: PathBuf) -> std::io::Result<ChosenFile> {
        let meta = std::fs::metadata(&path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let last_modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        Ok(ChosenFile {
            content_type: content_type_of(&name).map(str::to_string),
            relative_path: name.clone(),
            name,
            size: meta.len(),
            last_modified,
            source: FileSource::Path(path),
        })
    }

    /// Contents handed over whole, under `name`.
    pub fn from_bytes(name: impl Into<String>, bytes: Bytes, last_modified: u64) -> ChosenFile {
        let name = name.into();
        ChosenFile {
            content_type: content_type_of(&name).map(str::to_string),
            relative_path: name.clone(),
            name,
            size: bytes.len() as u64,
            last_modified,
            source: FileSource::Bytes(bytes),
        }
    }

    /// Every file under the folder `root`, depth first and by name, each
    /// with its path within it: what an input that chooses a folder holds.
    /// Hidden files (a name starting with a dot) are left out, as the
    /// pickers that show a folder's contents leave them out.
    pub fn from_folder(root: &Path) -> Vec<ChosenFile> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<ChosenFile>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let rel = format!("{prefix}{name}");
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    walk(&e.path(), &format!("{rel}/"), out);
                } else if let Ok(mut f) = ChosenFile::from_path(e.path()) {
                    f.relative_path = rel;
                    out.push(f);
                }
            }
        }
        let top = root
            .file_name()
            .map(|n| format!("{}/", n.to_string_lossy()))
            .unwrap_or_default();
        let mut out = Vec::new();
        walk(root, &top, &mut out);
        out
    }

    /// Its path on disk, where it has one.
    pub fn path(&self) -> Option<&Path> {
        match &self.source {
            FileSource::Path(p) => Some(p),
            FileSource::Bytes(_) => None,
        }
    }

    /// Its contents.
    pub fn read(&self) -> std::io::Result<Bytes> {
        match &self.source {
            FileSource::Path(p) => std::fs::read(p).map(Bytes::from),
            FileSource::Bytes(b) => Ok(b.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_is_read_as_the_standard_reads_it() {
        assert_eq!(
            Accept::parse(" .PNG, image/*,application/pdf , , bogus, /x, .a.b"),
            vec![
                Accept::Extension("png".into()),
                Accept::Group("image".into()),
                Accept::Mime("application/pdf".into()),
            ]
        );
    }

    #[test]
    fn types_become_extensions_to_filter_by() {
        let r = FileRequest::from_attributes(Some("image/png,.kdbx,image/jpeg"), false, false);
        assert_eq!(r.extensions(), ["png", "kdbx", "jpg", "jpeg"]);
        let images = FileRequest::from_attributes(Some("image/*"), true, false);
        assert!(images.extensions().contains(&"webp".to_string()));
        assert_eq!(images.filter_name(), "Images");
        assert_eq!(
            FileRequest::from_attributes(Some(".kdbx"), false, false).filter_name(),
            ".kdbx"
        );
    }

    #[test]
    fn a_type_with_no_known_extension_offers_any_file() {
        let r = FileRequest::from_attributes(Some(".md,application/x-unknown"), false, false);
        assert!(r.extensions().is_empty());
        assert_eq!(r.filter_name(), "All files");
        assert!(FileRequest::default().extensions().is_empty());
    }

    #[test]
    fn a_chosen_file_is_checked_against_accept() {
        let r = FileRequest::from_attributes(Some("image/*,.kdbx"), false, false);
        let file = |name: &str| ChosenFile::from_bytes(name, Bytes::new(), 0);
        assert!(r.accepts(&file("a.PNG")));
        assert!(r.accepts(&file("Passwords.kdbx")));
        assert!(!r.accepts(&file("notes.txt")));
        assert!(FileRequest::default().accepts(&file("anything")));
    }

    #[test]
    fn a_file_on_disk_and_a_folder() {
        let dir = std::env::temp_dir().join(format!("blitz-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.md"), "# b").unwrap();
        std::fs::write(dir.join("sub/a.png"), [0u8; 3]).unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        let f = ChosenFile::from_path(dir.join("b.md")).unwrap();
        assert_eq!((f.name.as_str(), f.size), ("b.md", 3));
        assert_eq!(f.content_type.as_deref(), Some("text/markdown"));
        assert_eq!(&f.read().unwrap()[..], b"# b");
        let top = dir.file_name().unwrap().to_string_lossy().into_owned();
        let all: Vec<_> = ChosenFile::from_folder(&dir)
            .into_iter()
            .map(|f| f.relative_path)
            .collect();
        assert_eq!(all, [format!("{top}/b.md"), format!("{top}/sub/a.png")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
