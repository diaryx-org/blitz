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

Not `dioxus-native` or `dioxus-native-dom`: this repository's copies are an
older line (0.7) than the published 0.8 alphas, which stay crates.io's.

## Keeping it

- **An upstream fix** we want before its release is cherry-picked with `-x`,
  its author kept, and a note in its message of what the backport changed.
- **A new Blitz release** that dioxus-native moves to: `leaf` is rebased onto
  its tag. A change upstream now has is dropped then. The winit backends in
  `patches/` move with it, to the winit that release pins, and
  `anyrender_vello`, `anyrender_vello_hybrid`, `glifo`, `vello_gpu` and
  `vello_gpu_shaders` to the anyrender and vello it takes; each goes when its fix is in that release.
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

Events and layout, found by diaryx-org/unit's gallery:

- A press focuses what it lands on when that can be focused, and a click goes
  to the nearest element the press and the release share.
- A node removed under `display: none` is forgotten by its layout parent.

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
lifecycle iOS 27 requires). They came from leaf's `patches/`.

And `anyrender_vello`, from Diaryx's app: a default frame latency of 2, not
1, so that on Metal a frame does not wait for the one on screen before it can
draw (about 22 ms a frame on an iPhone, before). dioxus-native gives no way
to set it.

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

And a package upstream does not have, `blitz-tui` (on the `tui` branch until
it merges into `leaf`): Blitz's laid-out page painted into a terminal's
character cells, a sibling of `blitz-paint` and `blitz-shell` that keeps the
DOM, Stylo and taffy as they are. It runs any `blitz_dom::Document`, a Dioxus
app included (`examples/tui_counter`), and is the renderer of leaf's terminal
editor (`apps/leaf-dioxus-tui` there). Its README says what it draws and what
it does not yet. Not `dioxus-tui`, the name DioxusLabs' retired terminal
renderer has on crates.io.
