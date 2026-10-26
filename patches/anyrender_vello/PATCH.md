# anyrender_vello 0.15.0, patched

The crate dioxus-native 0.8.0-alpha.3 takes for its `vello` renderer, kept
here beside the Blitz it draws. It came from diaryx-org/diaryx's
`apps/diaryx/patches/anyrender_vello`, where it was vendored on 2026-10-18;
`git log -p -- patches/anyrender_vello` here is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. Its manifest is
the published one, which takes anyrender and the other anyrender crates from
crates.io, so this crate alone is patched: none of its siblings need to be.

## What was wrong

`VelloRendererOptions` defaults `desired_maximum_frame_latency` to 1, and
dioxus-native's `RendererOptions` has no way to set it (it sets only the
features and limits on the options it builds). On Metal, wgpu gives the
layer one drawable more than the latency, so two: one on screen and one to
draw into. Every frame then waited to acquire a drawable until the one on
screen was let go. A Metal System Trace of Diaryx's app on an iPhone
(2026-10-18) showed the main thread waiting a median 22 ms a frame for its
drawable, 10.9 s of a 20 s recording, while the GPU's own work was about
5 ms: scrolling ran at about 27 frames a second on a 60 Hz display.

## What the patch does

The default is 2: three drawables, so a frame is drawn while one waits and
one is shown. The cost is up to a frame more between input and the screen,
and one more drawable's memory. `src/window_renderer.rs`, marked
`PATCH(diaryx)`.

anyrender_vello_hybrid, the renderer for the iOS Simulator, has the same
default of 1 and is not patched.

## No GPU

`VelloWindowRenderer::resume` called `.expect` on the surface, the
`DeviceHandle` (adapter and device), the `SurfaceRenderer` and Vello's
renderer. With no GPU to draw with, which an agent testing Diaryx in a Linux
virtual machine found, the app panicked as its window opened.

Each of these now falls back to the CPU: the renderer holds a
`VelloCpuWindowRenderer` (anyrender_vello_cpu, through softbuffer) for the
window from then on, through `suspend` and `resume`, and says why on stderr
once. Its `ScenePainter` is now `VelloWindowScenePainter`, an enum of Vello's
painter and vello_cpu's (`src/software.rs`); callers that name it as
`<VelloWindowRenderer as WindowRenderer>::ScenePainter`, as dioxus-native and
leaf-dioxus do, see no difference. `is_software()` says which one draws.
Custom wgpu textures cannot be registered in the fallback.

The fallback is in this crate, not in Blitz, because dioxus-native chooses the
renderer type at compile time from its features: blitz-shell is handed a
`VelloWindowRenderer` and cannot swap it. On the web the error is now the
panic's message, and there is no fallback.

This part goes when anyrender_vello falls back or reports the error itself,
so that dioxus-native or the app can choose; it is the change to propose
upstream (DioxusLabs/anyrender).

## When it goes

When dioxus-native lets the frame latency be set (a field on its
`RendererOptions`), or anyrender_vello defaults it higher, and dioxus-native
moves to that release; an app then sets it there. DioxusLabs/anyrender#93
made it configurable, defaulting to 1.
