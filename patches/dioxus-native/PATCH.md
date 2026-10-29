# dioxus-native 0.8.0-alpha.3, patched

The published dioxus-native that the org's apps launch with, kept here beside
the Blitz it runs. `git log -p -- patches/dioxus-native` here is the patch;
the first commit is the crates.io source as it was published.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. Its manifest is
the published one, which takes dioxus-native-dom and the Blitz crates by
version, so the consumer's patches of those apply to it as to the original.
This repository's own `packages/dioxus-native` is an older line (0.7) and is
not this one.

## What was wrong

An app could not open a second window. blitz-shell keeps any number
(`BlitzApplication::windows`), but dioxus-native made only the one it was
launched with: everything a window is made from (the root contexts, the net
provider, the HTML parser, the fonts, the renderer's options) was used up in
`launch_cfg_with_props`, and the contexts every window's `VirtualDom` is
given (the document, the window, the renderer, history, the event handlers)
only for that first window, in `can_create_surfaces`. `add_window` passed a
window to blitz-shell without them, and only before the event loop started.

Diaryx's desktop app wanted a window for each screen, as a desktop does,
rather than a grid of screens inside one.

## What the patch does

`dioxus_native::open_window(app, props, attributes)` asks for another
window, running `app` in a `VirtualDom` of its own. It queues the request on
the event loop's thread and wakes the loop with a new
`DioxusNativeEvent::OpenWindows`, which makes each window then, with the
active event loop: the window is made as the first one is, from the same
root contexts, providers, fonts and renderer options.

- `src/windows.rs`, new: the `Template` kept from launch, and
  `open_window`.
- `src/lib.rs`: `launch_cfg_with_props` builds the template and makes the
  first window from it.
- `src/dioxus_application.rs`: making a window and giving it its contexts
  is `init_window`, used for the first window and for each opened one;
  `OpenWindows` handled. `DioxusNativeApplication::new` is unchanged; the
  template is set with `with_template`, which only launch calls.

Closing a window drops it and ends the app only when it was the last, as
blitz-shell already did. Every window's `VirtualDom` runs on the event
loop's thread, so state shared between windows can live in a
`thread_local`.

## When it goes

When dioxus-native can open a window from a running app (upstream
DioxusLabs/dioxus, `packages/native`), and the org moves to that release.
This is the change to propose there.
