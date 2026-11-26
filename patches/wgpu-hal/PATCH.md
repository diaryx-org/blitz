# wgpu-hal 30.0.1, with Redox's Orbital in its EGL backend

The wgpu-hal vello 0.11 and anyrender_vello draw with, patched so wgpu runs
on Redox's GPU: its GLES backend over Mesa's EGL, whose Orbital platform the
Redox fork of Mesa carries. It is vendored unmodified in the commit before the
patch. One of four crates patched together, for the same reason: this one,
`wgpu`, `wgpu-core` and `wgpu-core-deps-windows-linux-android` (each with a
PATCH.md naming its change). Each is patched only where `target_os = "redox"`:
on every other platform the code built is the published code.

Measured on the M3 Pro (asahi, "Apple M3 Pro (G15S B1)", desktop GL 4.6
core): vello's GPU renderer animated in an Orbital window at about 100 frames
a second, and wgpu's own triangle at 103 to 108 (the redox-m3 tree's
`WGPU-REDOX.md`).

## The change (`src/gles/egl.rs`)

- `WindowKind::Orbital`.
- The display: a `RawDisplayHandle::Orbital`, or no display handle at all
  (as `wgpu_context` makes its instance), is Orbital's. Mesa's Orbital
  platform has no `EGL_EXT_platform_*` enum, so it is reached through
  `eglGetDisplay(EGL_DEFAULT_DISPLAY)`, with `EGL_PLATFORM=orbital` set if
  it is unset.
- The surface: a `RawWindowHandle::Orbital` is taken, and its `window` (the
  Orbital window's fd, which winit-orbital gives) is the
  `EGLNativeWindowType`, made into a surface with EGL 1.4's
  `eglCreateWindowSurface`.

## What a program needs besides

- To be linked dynamically: `-C target-feature=-crt-static`. Redox's Rust
  target links statically, and relibc's static programs have no `dlopen`
  ("dlfcn not supported"), which wgpu-hal loads `libEGL.so.1` with.
- Mesa's libraries where the loader finds them (`LD_LIBRARY_PATH`), built
  with the AGX3 fix for subgroup scans (Mesa fork, `ef756fe`): without it
  Mesa drops vello's dispatches and vello draws only its base colour.

Keep the four until wgpu builds its GLES backend on Redox itself.
