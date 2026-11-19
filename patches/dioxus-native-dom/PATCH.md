# dioxus-native-dom 0.8.0-alpha.3, patched

The published dioxus-native-dom, which turns Blitz's events into Dioxus's,
kept here beside the Blitz it runs on. `git log -p --
patches/dioxus-native-dom` is the patch; the first commit is the crates.io
source as it was published.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from:

```toml
dioxus-native-dom = { git = "https://github.com/diaryx-org/blitz", branch = "leaf", version = "0.8.0-alpha.3" }
```

Its `version` picks it out from this repository's older
`packages/dioxus-native-dom` (0.7), of the same name. Every app on this
fork's `leaf` needs it, not only one that uses a file
input: `leaf`'s blitz-traits has events the published crate does not match
(it matches Blitz's `DomEventData` exhaustively), so the published crate no
longer compiles against it.

## What was wrong

A page could not use `<input type=file>`. Blitz's own input chose files
(through blitz-shell's picker) and told the page nothing, and
`HasFileData::files` here was `vec![]` in any case: `onchange` never fired,
and an `oninput` that did had no files. `oncancel` was `unimplemented!()`.

## What the patch does

- `src/dioxus_document.rs`: Blitz's `input` and `change` become Dioxus's
  form events, with the input's files (`BlitzInputEvent::files`), and its
  `cancel` Dioxus's.
- `src/events.rs`: `NativeFormData::files`, each a `NativeFile` over Blitz's
  `ChosenFile`: its name, size, type, time, path (or its path within the
  folder chosen, for a file the system handed over whole), and its contents,
  read when the page asks (`read_bytes`, `read_string`, `byte_stream`).
  `NativeCancelData` for `oncancel`.
- `Cargo.toml`: `bytes`, the type Dioxus reads a file into.
