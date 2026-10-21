# vello_gpu_shaders 0.3.0, patched

The shaders vello_gpu 0.3 draws with, kept here beside `patches/vello_gpu`,
which this patch serves. It was vendored unmodified from crates.io on
2026-10-21 (linebender/vello at f3000c8d, `vello_gpu_shaders/`); `git log -p
-- patches/vello_gpu_shaders` here is the patch.

Consumers take it with `[patch.crates-io]` beside `patches/vello_gpu`, from
this repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. vello_gpu's
patched color matrix pass needs it: without it the pass is unknown to the
shader, and a color matrix draws transparent.

## What the patch does

`shaders/filter.wesl` gains a filter pass, `PASS_COLOR_MATRIX` (9), which
applies the 4x5 matrix stored after a filter's own record to each pixel, as
SVG's `feColorMatrix` does: to the unpremultiplied color, clamped to [0, 1].
`vello_gpu/src/filter.rs` writes the matrix and plans the pass; its
`PATCH.md` says why. The WESL is linked to WGSL for wgpu and, with `glsl`,
compiled to GLSL for WebGL, so both draw it: the crate's 14 tests pass with
`--features glsl`. Marked `PATCH(diaryx)`.

## When it goes

With `patches/vello_gpu`'s color matrices.
