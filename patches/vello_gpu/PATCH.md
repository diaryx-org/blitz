# vello_gpu 0.3.0, patched

The renderer under anyrender_vello_hybrid 0.11, Blitz's default `vello-hybrid`
renderer, kept here beside the Blitz it draws. It was vendored unmodified
from crates.io on 2026-10-20 (linebender/vello at f3000c8d, `vello_gpu/`);
`git log -p -- patches/vello_gpu` here is the patch.

Consumers take it with `[patch.crates-io]`, beside
`patches/anyrender_vello_hybrid` and `patches/glifo`, from this repository's
branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. Its manifest is the
published one, which takes vello_common and vello_gpu_shaders from crates.io;
neither is patched.

## What was missing

CSS `backdrop-filter`: an element whose border box shows what is beneath it,
filtered, a frosted panel. Blitz passes it down (`blitz-paint`'s effect
layer) and anyrender carries it to every backend, but no backend drew it, and
vello_gpu had no way to be asked. Vello's own tracking issue for Blitz
(linebender/vello#1491) lists it as not started.

## What the patch does

`Scene::push_layer_with_backdrop` pushes a layer as `push_layer` does, with a
backdrop filter recorded beside it by layer id (`Scene::backdrops`). When the
scheduler reaches the node that composes the layer, after the parent's draws
before it and before the layer itself (`Scheduler::schedule_backdrop`,
`src/schedule/mod.rs`):

1. The parent's pixels around the layer's clip, as far as the filter spreads,
   are drawn into a filter allocation of the opposite parity.
2. That allocation is filtered in place, by the same passes a filter layer
   takes (`schedule_filter`, the filter half of `schedule_layer` moved out
   unchanged).
3. It is drawn back into the parent through the layer's clip path, at the
   layer's opacity, and released.

The layer's own content is then composed over it as usual. Nothing new runs
on the GPU: the copy and the draw back are layer fills, the filter is a
filter op, so the WebGL backend draws it too. A backdrop needs a clip path,
the area it shows in; without one, none is drawn.

A backdrop at the root cannot read the user's surface, so it renders the
root into an intermediate layer, as a non-default blend at the root already
does (`root_is_blend_target`), and the intermediate textures are sized for a
filter allocation as large as the scene.

`build_draw` takes what a draw samples as a page and a ready point
(`SampledRegion`) rather than a scheduled layer, so a draw can sample a
parent. Marked `PATCH(diaryx)`. Three scheduler tests: a backdrop at the root
renders the root into a layer and filters once, a nested one reads its
parent, and one without a clip draws none. They read a font from the vello
repository's `assets/`, so they run in a checkout of linebender/vello: the
crate's 126 unit tests pass there.

Checked on a Mac, in unit's gallery built on `vello-hybrid`: a rounded panel
with `backdrop-filter: blur(12px)` over a gradient and cards, light and dark,
shows them blurred through its rounded corners, its own text sharp on top.

## What it does not do

- **The frame's cost.** A frame with a backdrop at the root draws the whole
  scene into an intermediate texture and blits it back, and loses the depth
  buffer's early-out for opaque paths at the root.
- **Edges.** The blur reads transparent beyond the parent's bounds (the
  window's edge, for the root), so a backdrop there darkens toward it; CSS
  would extend the edge pixels. vello_gpu's blur ignores edge modes.
- **Opacity and blending.** The backdrop takes the layer's opacity on its own,
  not as one group with the content, and not the layer's blend mode.
- **The backdrop root** is the nearest enclosing layer, as CSS's is the
  nearest ancestor with a filter, opacity, a mask or a blend mode; anyrender
  pushes a layer for exactly those, so the two agree.

## When it goes

When vello_gpu draws backdrop filters and anyrender_vello_hybrid passes them
on, and dioxus-native moves to those releases. Not reported upstream yet.
