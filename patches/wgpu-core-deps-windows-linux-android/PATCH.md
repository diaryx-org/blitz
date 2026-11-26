# wgpu-core-deps-windows-linux-android 30.0.1, on Redox too

One of four crates patched so wgpu runs on Redox's GPU; `../wgpu-hal/PATCH.md`
says why and what else a program needs. Vendored unmodified in the commit
before the patch.

- `Cargo.toml`: its `wgpu-hal` dependency is also taken on `target_os = "redox"`.
