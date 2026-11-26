# diaryx-org/blitz

A fork of [DioxusLabs/blitz](https://github.com/DioxusLabs/blitz), owned by
diaryx-org for [leaf](https://github.com/diaryx-org/leaf)'s Dioxus app and
whatever else the org builds on Blitz. Upstream does what it will; this fork
carries what we need now.

## Branches

- **`leaf`**: the branch every org app builds its Blitz stack from, leaf's
  and others' (the name is where it began). It starts at the release tag
  dioxus-native pins exactly (`v0.3.0-beta.4` for dioxus-native
  0.8.0-alpha.3), so its crates keep that version and can replace the
  published ones through `[patch.crates-io]`; our changes are commits on top.
- **`main`**: upstream's, kept as it is.

## Using it

```toml
[patch.crates-io]
blitz-dom = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
blitz-traits = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
blitz-shell = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
blitz-paint = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
stylo_taffy = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
debug_timer = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# and blitz-net, blitz-html, accesskit_xplat where the graph has them
# The winit backends, patched (patches/*/PATCH.md), for the platforms built:
winit-appkit = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
winit-uikit = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# The renderer, patched (patches/anyrender_vello/PATCH.md), with the `vello`
# feature: alone, as its manifest takes its siblings from crates.io
anyrender_vello = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# The SVG renderer blitz-paint draws an SVG <img> with, patched
# (patches/anyrender_svg/PATCH.md): alone, as anyrender_vello
anyrender_svg = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# Vello's shaders, patched (patches/vello_shaders/PATCH.md): no reads out of
# bounds, for the iPhone 12 Pro; alone, as anyrender_vello
vello_shaders = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# On Android, with accessibility: AccessKit's adapter, with a fix it has not
# released (patches/accesskit_android/PATCH.md)
accesskit_android = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# On Redox (Orbital): winit's backend with Orbital's HiDPI scale
# (patches/winit-orbital/PATCH.md), and, for the `vello` feature on the GPU,
# the four wgpu crates that build its GLES backend over Mesa's Orbital EGL
# (patches/wgpu-hal/PATCH.md). Built only for Redox; patched everywhere, as
# cargo cannot patch per target, where they are the published code.
winit-orbital = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
wgpu = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
wgpu-core = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
wgpu-core-deps-windows-linux-android = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
wgpu-hal = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
# Or, with the `vello-hybrid` feature, the four that draw emoji, backdrop
# filters and color matrices with it (patches/*/PATCH.md)
anyrender_vello_hybrid = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
glifo = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
vello_gpu = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
vello_gpu_shaders = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
```

Patch **every** crate of this repository that is in the graph, not only the
ones `leaf` changes: they depend on each other by path, so one taken from
crates.io beside them is a second copy of it, and the build fails on two
`blitz-traits`. Cargo warns of a patch the graph does not use; drop it.

Not this repository's `packages/dioxus-native` and `packages/dioxus-native-dom`:
those are an older line (0.7) than the published 0.8 alphas. The published
dioxus-native is patched here, in `patches/dioxus-native` (its `PATCH.md`),
for an app that opens more than one window; and the published
dioxus-native-dom, in `patches/dioxus-native-dom`, which every app needs,
as `leaf`'s events no longer compile with the published one (its
`PATCH.md`):

```toml
dioxus-native = { git = "https://github.com/diaryx-org/blitz", branch = "leaf", version = "0.8.0-alpha.3" }
dioxus-native-dom = { git = "https://github.com/diaryx-org/blitz", branch = "leaf", version = "0.8.0-alpha.3" }
```

Each with its `version`: the repository holds the older line under the same
names (`packages/`), and without it Cargo finds two candidates and stops.

## Keeping it

- **An upstream fix** we want before its release is cherry-picked with `-x`,
  its author kept, and a note in its message of what the backport changed.
- **A new Blitz release** that dioxus-native moves to: `leaf` is rebased onto
  its tag. A change upstream now has is dropped then. The winit backends in
  `patches/` move with it, to the winit that release pins, and
  `anyrender_vello`, `anyrender_svg`, `anyrender_vello_hybrid`, `glifo`, `vello_gpu`,
  `vello_gpu_shaders` and `vello_shaders` to the anyrender and vello it takes,
  and the four wgpu crates to the wgpu that anyrender takes; each goes when its fix is in that release.
- **A winit fix** goes upstream to winit, not Blitz (each `PATCH.md` says
  where it stands).
- Commits follow the diaryx-org rules: Conventional Commits, a
  `Behavioural-change:` trailer where a caller would notice.

## What is on `leaf`

Accessibility, found in leaf's Mac app and an iOS spike (now
`examples/ios-a11y`), where with these VoiceOver and agents read and press a
page:

- Bounds on every AccessKit node (DioxusLabs/blitz#842, cherry-picked), and the
  viewport's for the document node.
- A page's text as static text (a `Label`, its text the value); elements
  with no semantics as `GenericContainer`.
- The click action on buttons, links, menu items and choices.
- ARIA's states: checked and pressed as toggled, a value and its range,
  modal, orientation; the roles switch and separator.
- An action request carried out: a click as a click, through the window's
  own events (`View::click_events`, `View::accessibility_action_events`).
- A hook through which the embedder changes each tree built before the
  adapter sees it (`View::set_accessibility_tree_hook`): leaf's editor
  becomes a text area there, with its value, text runs and selection.
- The tree rebuilt after every redraw, where the changes are resolved:
  upstream rebuilds it on a poll that `has_changes`, which is never true
  after the first node is made, so the tree kept the first screen.
- A text input's text as its value (a password's as dots), its placeholder,
  and the `Focus` and `SetValue` actions, carried out by
  `View::accessibility_action`: `SetValue` focuses the input, selects all of
  it and commits the new text, which the page hears as an `input` event.

- On Android, the tree at all: blitz-shell's `accessibility` feature turns
  on `accesskit_xplat`'s Android adapter, which was off, so every Android
  app had the null one. The adapter injects AccessKit's delegate into a
  host view, GameActivity's surface view or one the embedder names with
  `accesskit_xplat::set_android_host_view` (a NativeActivity has no view to
  offer), and fails soft where it found neither. Found in Diaryx's Android
  app, where TalkBack reads and presses its screens with these and the
  `winit-android` and `accesskit_android` patches below.
- On the web, the tree at all: `accesskit_xplat` had no web adapter, so a
  page drawn on a canvas had the null one and assistive tech saw one box.
  `platform_impl/web.rs` mirrors the tree as transparent elements laid over
  the canvas at each node's bounds, with its ARIA role, name and states; the
  canvas is an `application` that owns them, with the focused node as its
  `aria-activedescendant`, so the keyboard stays with winit. A click assistive
  tech sends to an element comes back as the node's `Click`, or `Focus` where
  it has no click. Found in Diaryx's web build, checked with headless
  Chromium's accessibility tree.

Events and layout, found by diaryx-org/unit's gallery:

- A press focuses what it lands on when that can be focused, and a click goes
  to the nearest element the press and the release share.
- A node removed under `display: none` is forgotten by its layout parent.
- A `position: fixed` box contained by the root element stays where the
  viewport puts it while the document scrolls: drawn, hit and measured
  (`getBoundingClientRect`, element coordinates) there. Upstream tested for
  the root element as a box with no containing block, which dioxus-native's
  root element has (a node above it that is not an element), so a page's
  menus, dialogs and sheets scrolled off with the page.
- `<input type=file>` works, for a page and on every platform with a
  picker (`packages/blitz-shell/src/files.rs`). A press asks the shell for
  the system's picker and returns, so the window keeps drawing; upstream
  opened rfd's blocking dialog in the press, ignored `accept`, and told the
  page nothing. The input's `accept` (extensions, MIME types, `image/*`),
  `multiple` and `webkitdirectory` reach the picker, which opens on the
  window and in the folder the last file came from. The answer comes back
  as `UiEvent::FilesChosen`: the files, with their names, sizes, types and
  contents (`blitz_traits::files::ChosenFile`; a path, or the bytes a
  picker handed over), then `input` and `change`, or `cancel`, as a
  browser fires them; files `accept` does not allow are left out, and the
  `value` is a browser's (`C:\fakepath\…`). The page reads them through
  Dioxus's `files()` (`patches/dioxus-native-dom`). Found by a KeePass
  client on diaryx-org/unit's components, choosing a database.
- On Linux and the BSDs, a file input's picker is the desktop's own, through
  the XDG desktop portal's FileChooser, over a D-Bus client of our own
  (`packages/blitz-shell/src/files/dbus.rs`: the session bus, EXTERNAL
  authentication, and the wire format of the types the portal uses), not
  rfd's: `accept` as globs in either case and as MIME types, the last folder,
  the window as parent on X11; `zenity`, else `kdialog`, where there is no
  portal. Run here against a real dbus-daemon, xdg-desktop-portal-gtk and
  zenity 4.
- Return or Space on a focused file input opens its picker, as a press does.
  And an app opens a picker from its own code, for a menu's Open… or a
  shortcut, where there is no file input to press:
  `blitz_shell::choose_files(&window, request).await`, with a file input's
  defaults.
- On the web, with Blitz drawing into a canvas, a file input's picker is
  the browser's own: the page is given a hidden `<input type=file>` of its
  own with the input's `accept`, `multiple` and `webkitdirectory`, clicked
  while the press still lets a page open a picker, and its files are read
  whole (`packages/blitz-shell/src/files/web.rs`). Checked in Chromium:
  the press opened its chooser, and the file chosen reached the document.
- On Windows, a file input's picker is the Common Item Dialog
  (`IFileOpenDialog`), its COM interfaces declared by hand on windows-sys
  (`packages/blitz-shell/src/files/windows.rs`), not rfd's: owned by the
  window, on a thread and apartment of its own, `accept` as one filter, the
  last folder, folders for `webkitdirectory`. blitz-shell no longer takes
  rfd at all.
- On macOS, a file input's picker is AppKit's open panel of our own, not
  rfd's: a sheet on the window, `accept` as UTTypes (shared with iOS's,
  `packages/blitz-shell/src/files/uttypes.rs`), folders alone for
  `webkitdirectory`, the last folder.
- On iOS, a file input's picker is UIKit's document picker, presented over
  the window (`packages/blitz-shell/src/files/ios.rs`): `accept` as UTTypes,
  files taken as copies the app reads by path, a folder's files read while
  access to it is granted. rfd has no iOS picker, so upstream had none.
- An empty text input or textarea paints its `placeholder`, laid out in its
  own styles and faded to Firefox's `::placeholder` opacity (0.54); upstream
  gave it to the accessibility tree and never drew it, so a field showed
  nothing but its box.
- A password input (`type=password`) draws a bullet for each character,
  where upstream drew its text as typed and hid it only from assistive tech.
  Its editor holds only the bullets, so only they are laid out, painted and
  selected; the text is kept apart (`TextInputData::secret`), each edit's
  insertion reconciled into it by the caret before and after, in a buffer
  wiped when it changes and when it goes. It is the input's value (the
  `input` event's, a form's, a script's), and its `value` attribute keeps
  none of it once the input is laid out. It is not copied or cut, an input
  method's composition is not drawn, and a change of `type` in and out of
  `password` keeps its text and caret: a password shown and hidden. Found by
  a KeePass client on diaryx-org/unit's components.

From Diaryx, where leaf's editor cancels `mousedown` to keep its own caret
and selection, and the app's labels are `user-select: none`:

- A press whose `pointerdown` or `mousedown` was cancelled starts no text
  selection: its drag no longer carries an earlier press's anchor to the
  pointer, across everything between them.
- `user-select` is read as its used value, `none` holding for every
  descendant that does not say otherwise (upstream looked one parent up). A
  press on such text anchors nothing, a drag from it selects nothing, and
  the cursor over it is the arrow, not the text cursor.

From Diaryx's iOS and Mac app, which carried them as patches of the
published crates (`apps/diaryx/patches` there):

- The hit test and the inline fragment walk skip a node removed since the
  last layout, where they panicked on a pointer event between a mutation
  and the next layout.
- A `<style>`'s sheet goes into the stylist in document order, not by node
  id, which a reused slot made arbitrary.
- A touch fling slows as UIKit's scroll view does: the finger's own speed,
  0.998 of it kept a millisecond.
- The viewport is the whole window on every platform: the shell no longer
  subtracts the safe area or paints its margins white, and a page keeps
  clear of the notch itself (`Window::safe_area`).

And the winit backends dioxus-native pins, patched, in `patches/`:
`winit-appkit` (the Mac's text input client answers about the text round the
caret: accents, the emoji picker, dictation) and `winit-uikit` (the UIScene
lifecycle iOS 27 requires), which came from leaf's `patches/`;
`winit-android` (`Ime` events an application's own input connection hands
it, an event loop that ends with its activity, and hovers left to the
window's views, for TalkBack's explore-by-touch), from Diaryx's Android
app; and `winit-web` (an input method through a hidden textarea, which is
what raises a phone's keyboard, given only for a tap on text), from
Diaryx's web build.

And `accesskit_android`, the adapter `accesskit_xplat` takes on Android, with
AccessKit's unreleased fix for a crash on the first letter typed into an
empty field (AccessKit/accesskit#813).

And `dioxus-native` itself, in `patches/dioxus-native`: `open_window`, so
an app opens more windows than the one it was launched in, each running a
`VirtualDom` of its own. From Diaryx's desktop app, where a screen is a
window.

And in Blitz's text input, `Ime::DeleteSurrounding`, which was a `TODO`: the
text either side of the selection deleted, as an input method asks before a
correction or a recomposed word.

And `anyrender_vello`, from Diaryx's app: a default frame latency of 2, not
1, so that on Metal a frame does not wait for the one on screen before it can
draw (about 22 ms a frame on an iPhone, before). dioxus-native gives no way
to set it.

And `anyrender_vello` again: with no GPU to draw with (no adapter, device or
surface from wgpu, as in a Linux virtual machine), the window draws on the
CPU through vello_cpu and softbuffer instead of the app panicking. Blitz
cannot choose the renderer itself: dioxus-native fixes it at compile time.

And `anyrender_vello` a third time, from Diaryx's app on a 120 Hz Linux
screen: after presenting a frame it no longer waits for the GPU to finish
it, so the next frame's CPU work runs while the GPU draws (`PATCH.md`). A
frame took about 9.7 ms end to end before, past 120 Hz's 8.3 ms.

And `anyrender_svg`, from Diaryx's app: an SVG's raster image is decoded
once while its tree lives, not on every frame, and turned the way its EXIF
orientation says. A photo in leaf's Reflow view, drawn inside a piece's
SVG, had been decoded and uploaded again each frame, and lay on its side.
And in `blitz-dom`, a raster `<img>` is turned by its EXIF orientation too,
as a browser does by default.

And `vello_shaders`, from Diaryx's app: six reads of a workgroup array at
`ix - 1u` inside a `select`, which reads index 0xffffffff when `ix` is 0,
now index `max(ix, 1u) - 1u`. An iPhone 12 Pro's first frame stopped on a
GPU page fault in `tile_alloc` and the app stayed black. Vello compiles
every stage unchecked, as published; the shaders no longer need checks.

And `anyrender_vello_hybrid` and `glifo`, so that Blitz's default renderer
draws a bitmap glyph, which every Apple Color Emoji is: from glifo's atlas,
with `png` on, at any size and in synthesised italic. Before, an emoji drew
as nothing or panicked, and the org's apps built with classic `vello` for
it.

And `vello_gpu`, with `anyrender_vello_hybrid`, so that `vello-hybrid` draws
CSS `backdrop-filter`: what is beneath an element, filtered, through its
border box (`patches/vello_gpu/PATCH.md`). `blitz-paint` clips a backdrop to
the rounded border box rather than its bounding rectangle.

And `vello_gpu_shaders`, with `vello_gpu` and `anyrender_vello_hybrid`, so
that `vello-hybrid` draws color matrices (CSS's `brightness()`, `contrast()`
and the rest) and a filter of a blur and a matrix after it: blurred shapes
that merge, the "gooey" effect.

And `filter` and `backdrop-filter` naming an SVG `<filter>` in the page,
through `--blitz-filter: url(#id)` and `--blitz-backdrop-filter: url(#id)`
beside the standard `url(#id)`, which Stylo's Servo build cannot parse
(`blitz-paint/src/filters.rs`). Its `feGaussianBlur`, `feColorMatrix` and
`feOffset` are read as one chain, enough for blurred shapes that merge.

And fonts for the text inside SVG images, which usvg sets from a font
database of its own holding only the system's fonts: `add_svg_fonts` adds a
host's, and a generic family the database lacks falls back to the first of
them. In a browser, which lists no system fonts, an SVG's text drew as
nothing before; found in leaf's web host, where a Mermaid diagram's labels
are SVG text.

And a wheel's pixel delta in CSS pixels, from Diaryx's app on Linux: winit
gives it in physical pixels, every backend converting its logical delta with
the scale factor, and blitz-shell passed it on as CSS pixels, so on a 2x
screen a trackpad scrolled a page twice as far as the fingers went. It is
divided by the scale factor now. A line delta is unchanged.

And on Linux and the BSDs, where the platform leaves it to the app, the
wheel and the trackpad scroll a page once a frame (`blitz-shell/src/wheel.rs`),
from Diaryx's app on a 120 Hz screen. Upstream dispatched each wheel event as
it came, so a page moved as often as the device reported, unevenly against
the display, a notch jumped a line at once, and a trackpad stopped dead when
the fingers lifted. Now a trackpad's path is replayed a report behind and
sampled at each frame, a notch is eased over about 75 ms, and lifted fingers
glide on, slowing as the touch fling does, until two fingers set down, a
press, the pointer moving or new scrolling stop them. A Ctrl- or Cmd-wheel goes through as it
comes. macOS sends momentum of its own and is left as it was.

And a package upstream does not have, `blitz-tui` (on the `tui` branch until
it merges into `leaf`): Blitz's laid-out page painted into a terminal's
character cells, a sibling of `blitz-paint` and `blitz-shell` that keeps the
DOM, Stylo and taffy as they are. It runs any `blitz_dom::Document`, a Dioxus
app included (`examples/tui_counter`), and is the renderer of leaf's terminal
editor (`apps/leaf-dioxus-tui` there). Its README says what it draws and what
it does not yet. Not `dioxus-tui`, the name DioxusLabs' retired terminal
renderer has on crates.io.
