# winit-orbital 0.31.0-beta.3, with Orbital's HiDPI scale

The crate dioxus-native pins, patched for Diaryx's app on Redox (Orbital) on
a HiDPI panel, and kept here beside the Blitz it is built with. It is
vendored unmodified in the commit before the patch. Published, it never asks for a scale and answers 1 from
`scale_factor()`, so Blitz lays the app out in pixels and on a 3024x1964
panel every point is one pixel: the app draws at about half size.

Orbital's protocol for a window that can scale: opened with the flag `s`, the
window is sent a `ScaleEvent` (`scale / baseline`, 276/160 on that panel), and
the pointer in its coordinates divided by that scale. Orbital draws the
window's buffer as it is: the app draws in pixels.

- `window.rs`: every window is opened with `s`; `scale_factor()` answers the
  scale last sent. `request_surface_size` answers `Some(size)`: Orbital
  resizes at once and sends no `ResizeEvent` for a size the app set, so the
  published `None` (an event to come) never came.
- `event_loop.rs`: a `ScaleEvent` becomes `ScaleFactorChanged`, with the
  window's pixels grown by the change (its size in points kept) unless it is
  maximized or fullscreen; the size the app settles on is set and given as
  `SurfaceResized`, then a redraw. The pointer is multiplied back to pixels.
- `lib.rs`: the scale is kept on the window's socket, shared by the window
  and the event loop.

Only a Redox build compiles it. Keep it until winit's Orbital backend does
this itself (upstream's `winit-orbital` still answers 1).
