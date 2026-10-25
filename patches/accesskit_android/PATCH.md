# accesskit_android 0.9.0, patched

The Android adapter `accesskit_xplat` takes on Android, kept here beside it
until AccessKit releases what it carries. It is vendored unmodified in the
commit before the patch, with AccessKit's two licence files beside it.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`.

## What was wrong

- **Typing into an empty field aborted the app.** The text-changed event
  took one from the old text's length as an unsigned number, which
  overflowed for an empty field; the panic was on Android's UI thread,
  inside a JNI call, and so ended the process. Found in Diaryx's first-run
  screen, the first letter of a name typed with TalkBack on.

## What the patch does

- AccessKit's own fix, AccessKit/accesskit#813 (`115887d84`), cherry-picked
  with its author: the lengths are signed before one is taken from them.

## Upstream

Fixed on AccessKit's `main` and unreleased (the newest release is 0.9.0).
Drop this copy, and the consumers' patch of it, for the release that has it.
