# winit-android 0.31.0-beta.3, patched

The crate dioxus-native pins, patched for Diaryx's Android app and kept
here, in diaryx-org's Blitz fork, beside the Blitz it is built with. It is
vendored unmodified in the commit before the patch. Every change is marked
`leaf patch` in the source.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }` — cargo finds the
crate by name in the repository.

## What was wrong

- **No input method events.** A `NativeActivity` gives an input method no
  `InputConnection`, so winit-android reports no `WindowEvent::Ime` at all,
  and a soft keyboard can only send keys: no accents, emoji, suggestions,
  corrections, swipe or voice typing. An application can give the input
  method a connection of its own (a Java `View` over the activity, with
  `onCreateInputConnection`), but had no way to hand winit what it heard.
- **The event loop outlived its activity.** `MainEvent::Destroy` was a
  `TODO`: the loop went on after the activity was gone, in a process Android
  keeps for the next activity. When the app was launched again, Android
  called `android_main` again in that process, where winit refuses a second
  event loop (`RecreationAttempt`), and the app never drew past its splash
  screen.

## What the patch does

- `winit_android::ime::send(Ime)` (`ime.rs`): queue an `Ime` event from any
  thread and wake the event loop, which delivers it to the window as
  `WindowEvent::Ime` in the next iteration, after the input queue's events.
  A wake with an event queued is not ignored as a spurious one.
- On `MainEvent::Destroy`, the event loop exits, so `run_app` returns and the
  application is dropped. An application ends its process there, or keeps
  it and makes nothing more of winit.

An application that never calls `ime::send` gets the old behaviour, except
that the event loop now ends with its activity.

## Upstream

Not prepared. Upstream winit has the same `TODO` for `Destroy`; the `Ime`
queue is a stopgap for winit giving the Android backend an input connection
of its own (GameActivity's `TextInputState` covers it for that activity).
