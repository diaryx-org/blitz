# wgpu 30.0.1, with its GLES backend on Redox

One of four crates patched so wgpu runs on Redox's GPU; `../wgpu-hal/PATCH.md`
says why and what else a program needs. Vendored unmodified in the commit
before the patch.

- `build.rs`: the `gles` cfg alias is also set on `target_os = "redox"`.
