# vello 0.11.0, patched

The renderer anyrender_vello draws with, kept here beside the Blitz that
calls it. Vendored from crates.io on 2026-10-30; `git log -p --
patches/vello` here is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. Its manifest is
the published one, which takes vello_encoding, vello_shaders and wgpu from
crates.io, so this crate alone is patched.

## What was wrong

Vello compiles every shader with `wgpu::ShaderRuntimeChecks::unchecked()`:
no bounds checks, so a shader that indexes past an array reads or writes
whatever is there. Two of its stages do:

- `tile_alloc` reads `sh_tile_count[local_id.x - 1u]` inside
  `select(0u, ..., local_id.x > 0u)`. WGSL's `select` evaluates both arms,
  so thread 0 of every workgroup reads workgroup memory at index
  0xffffffff.
- The MSAA variants of `fine` (`fine_msaa8`, `fine_msaa16`) do the same
  with `sh_count[el_ix - 1u]`, twice.

An iPhone 15 Pro draws through it. An iPhone 12 Pro (A14) does not: its
first frame stops on a GPU page fault in `tile_alloc`
(`kIOGPUCommandBufferCallbackErrorPageFault`), the command buffers after it
are ignored, and the app shows a black screen. Found in Diaryx's app built
for that phone on 2026-10-30, with classic `vello`.

Tried on the 12 Pro, wgpu's checks turned on stage by stage:

| Checked | Page fault | Picture |
|---|---|---|
| none | yes | corrupt, then black |
| every stage | no | clean |
| every stage, bounds checks only | no | clean |
| every stage, loop bounding or division checks only | yes | black |
| every stage but `tile_alloc` (several splits) | yes | black |
| `tile_alloc` | no | text and shapes torn into rows |
| `tile_alloc`, `fine_msaa16` | no | clean |

`coarse` and `backdrop_dyn` have the same `select` but drew cleanly
unchecked there.

## What the patch does

`WgpuEngine::create_compute_pipeline` gives `tile_alloc`, `fine_msaa8` and
`fine_msaa16` `ShaderRuntimeChecks::checked()` and the rest of the stages
`unchecked()`, as before (`src/wgpu_engine.rs`, marked `PATCH(diaryx)`).
Checks on every stage cost 3.2 ms a frame on the 15 Pro (render p50 6.5 to
9.7 ms), past a 120 Hz frame; what these three cost there is still to be
measured.

The fix that belongs upstream is in the shaders: read the neighbour's count
only when there is one, so no check is needed. Not reported yet.
