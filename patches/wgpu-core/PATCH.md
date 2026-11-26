# wgpu-core 30.0.1, with its GLES backend on Redox

One of four crates patched so wgpu runs on Redox's GPU; `../wgpu-hal/PATCH.md`
says why and what else a program needs. Vendored unmodified in the commit
before the patch.

- `build.rs`: the `gles` cfg alias is also set on `all(target_os = "redox",
  feature = "gles")`. Vulkan stays off: Redox has no Vulkan driver yet.
- `Cargo.toml`: `wgpu-core-deps-windows-linux-android`, which brings
  wgpu-hal with its GLES backend, is also a dependency on Redox.
