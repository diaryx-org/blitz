# anyrender_vello_hybrid 0.11.0, patched

The crate dioxus-native 0.8.0-alpha.3 takes for its default `vello-hybrid`
renderer, kept here beside the Blitz it draws. It was vendored unmodified
from crates.io on 2026-10-20 (DioxusLabs/anyrender at cf0f1477,
`crates/anyrender_vello_hybrid`); `git log -p --
patches/anyrender_vello_hybrid` here is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`, together with
`patches/glifo` and `patches/vello_gpu`: this crate alone draws an ordinary
emoji, glifo's patch covers the rest, and vello_gpu's draws a backdrop filter
(each `PATCH.md`). Its manifest is the published one, which
takes anyrender, vello_gpu and glifo from crates.io, so none of its siblings
need patching.

## What was wrong

A bitmap glyph drew as nothing, or panicked. Apple Color Emoji is all
bitmap glyphs (PNG strikes, `sbix`), so every emoji did, and the org's apps
built with classic `vello` instead to draw them (leaf's
`crates/leaf-dioxus/Cargo.toml` says so), giving up the renderer Blitz
defaults to, the one the iOS Simulator can run, and CSS `filter`.

Two things, both here:

- glifo decodes a PNG strike only with its `png` feature, which nothing in
  dioxus-native's graph turned on, so every bitmap glyph was skipped.
- With `png` on, glifo drew each bitmap glyph as a pixmap paint, because its
  glyph atlas is off unless a run asks for it, and `draw_glyphs` never did.
  vello_gpu cannot draw a pixmap paint: it panics, "pixmap image sources are
  not supported by Vello GPU". It can draw one from the atlas.

## What the patch does

`Cargo.toml` takes glifo with `png`, and `draw_glyphs` turns the atlas on
for every run, filled or stroked, in `src/scene.rs` and
`src/webgl_scene.rs`, marked `PATCH(diaryx)`. An outline glyph is cached in
the atlas too, then, at a size of 128 or less: glifo's own default, and what
vello_gpu's caching is for.

Checked on a Mac with unit's gallery built on `vello-hybrid`: a face, a
party popper and a thumbs-up with a skin tone in a heading, and with
`patches/glifo`, the same in italic and a crab at 160 px. Without the
patch, the first frame panics.

## Backdrop filters

`push_layer` passes a layer's backdrop filter to vello_gpu's
`push_layer_with_backdrop`, with the layer's clip shape as its clip path,
the area the backdrop shows in (`src/scene.rs`, `src/webgl_scene.rs`,
`PATCH(diaryx)`). It needs `patches/vello_gpu`; see its `PATCH.md`.

## What it does not do

The atlas grows to 8 pages of 4096 × 4096; a bitmap glyph that finds none
of them with room still falls back to a pixmap paint, and panics. An emoji
strike is at most 160 px square.

The frame latency is still 1 here, as `patches/anyrender_vello` changed for
classic vello.

## When it goes

When anyrender_vello_hybrid turns the atlas on and takes glifo with `png`,
and dioxus-native moves to that release. Neither is reported upstream yet.
