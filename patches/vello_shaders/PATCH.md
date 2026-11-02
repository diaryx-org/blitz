# vello_shaders 0.11.0, patched

The shaders vello 0.11 draws with. Vendored unmodified from crates.io on
2026-11-02; `git log -p -- patches/vello_shaders` here is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. vello itself is
the published one, and compiles the WGSL this crate's `build.rs` embeds.

## What was wrong

Six places read a workgroup array at the previous index inside a `select`:

```wgsl
let tile_subix = select(0u, sh_tile_count[local_id.x - 1u], local_id.x > 0u);
```

WGSL's `select` evaluates both arms, so when the index is 0 the shader reads
`sh_tile_count[0xffffffff]`, out of bounds. Vello compiles every shader with
`wgpu::ShaderRuntimeChecks::unchecked()`, so nothing clamps the read:

- `tile_alloc.wgsl`, `sh_tile_count[local_id.x - 1u]`.
- `fine.wgsl`, `sh_count[el_ix - 1u]`, twice (the MSAA variants,
  `fine_msaa8` and `fine_msaa16`).
- `coarse.wgsl`, `sh_part_count[part_ix - 1u]` and
  `sh_tile_count[el_ix - 1u]`.
- `backdrop_dyn.wgsl`, `sh_row_count[el_ix - 1u]`.

An iPhone 15 Pro draws through it. An iPhone 12 Pro (A14) does not: its
first frame stops on a GPU page fault in `tile_alloc`
(`kIOGPUCommandBufferCallbackErrorPageFault`), and with `tile_alloc` checked,
`fine_msaa16` still tears text into rows. `coarse` and `backdrop_dyn` drew
cleanly unchecked there, but read out of bounds the same way. Found in
Diaryx's app on 2026-10-30, with classic `vello`.

The first fix (blitz#26) compiled `tile_alloc`, `fine_msaa8` and
`fine_msaa16` with wgpu's runtime checks, in a vendored vello. On an iPhone
15 Pro that cost 2.9 ms a frame (render p50 6.6 to 9.5 ms, GPU wait 4.6 to
8.0 ms), past a 120 Hz frame: nearly as much as checks on every stage.

## What the patch does

Each of the six reads indexes `max(ix, 1u) - 1u` instead of `ix - 1u`.
Where `ix > 0u` that is the same element; where it is 0 the read is of
element 0 and the `select` discards it, as before. No runtime checks are
needed, and vello and wgpu are the published ones. Marked `PATCH(diaryx)`.

## When it goes

When vello releases the same fix. Not reported upstream yet.
