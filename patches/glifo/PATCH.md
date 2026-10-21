# glifo 0.4.0, patched

The glyph renderer vello_gpu 0.3 draws text with, under
anyrender_vello_hybrid. It was vendored unmodified from crates.io on
2026-10-20 (linebender/vello at f3000c8d, `glifo/`); `git log -p --
patches/glifo` here is the patch.

Consumers take it with `[patch.crates-io]`, beside
`patches/anyrender_vello_hybrid`, from this repository's branch the rest of
their Blitz comes from, `{ git = "https://github.com/diaryx-org/blitz",
branch = "leaf" }`. Classic `vello` does not use glifo, so a build with it
is unchanged; Cargo warns the patch is unused there.

## What was wrong

vello_gpu can draw a bitmap glyph (an emoji) only from glifo's atlas: drawn
uncached, it is a pixmap paint, and vello_gpu panics. glifo kept a bitmap
glyph out of the atlas in two cases where nothing about the bitmap needs it:

- **A run above `max_cached_font_size`** (128). The bound is for outlines,
  which are rasterised at the run's size. A bitmap is cached at its strike's
  own size, which its key holds, and is scaled to the run's when drawn.
- **A transform with a skew**, or a mirror. Blitz skews a run in synthesised
  italic, and Apple Color Emoji has no italic, so any emoji in italic text
  panicked. The atlas holds the bitmap's own pixels and is drawn as an image
  through the glyph's transform, exactly as the uncached pixmap is, so a skew
  or a mirror draws the same from either.

## What the patch does

A bitmap glyph is cached whenever the atlas is on (`src/glyph.rs`), under
any transform that does not flatten it (`supports_atlas_caching`,
`src/renderer.rs`); outlines and COLR glyphs keep both bounds. Marked
`PATCH(diaryx)`. Tests: `bitmap_allows_any_invertible_transform` replaces
the bitmap half of `rejects_skewed_transforms` and
`bitmap_allows_positive_scales_only`, and
`large_or_skewed_bitmap_glyph_is_cached` draws a strike at 200 px and in a
skew and finds one atlas entry. It fails without the patch. The tests read
fonts from the vello repository's `assets/`, so they run in a checkout of
linebender/vello, not here: `cargo test --features png` there passes, 27
tests.

## When it goes

When glifo caches a bitmap glyph regardless of the run's size and of a
skew, or vello_gpu draws a pixmap paint, and vello_gpu and
anyrender_vello_hybrid move to that release. Not reported upstream yet.
