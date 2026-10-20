# winit-uikit 0.31.0-beta.3, patched

The crate dioxus-native pins, kept here, in diaryx-org's Blitz fork, beside
the Blitz it is built with. It came from diaryx-org/leaf's
`patches/winit-uikit`, where it was vendored unmodified ("chore(patches):
vendor winit-uikit 0.31.0-beta.3 unmodified") and patched ("fix(patches):
adopt the UIScene lifecycle in winit-uikit, for iOS 27"): `git log -p --
patches/winit-uikit` there is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, as they
take `../winit-appkit`, from this repository's branch the rest of their
Blitz comes from, `{ git = "https://github.com/diaryx-org/blitz", branch =
"leaf" }`.

## What was wrong

An app built with the iOS 27 SDK must adopt the UIScene lifecycle, declaring
a scene configuration in its `Info.plist`; iOS 27 refuses to launch one that
does not. winit-uikit 0.31.0-beta.3 knows nothing of scenes: it makes its
window from the application's lifecycle, which a scene app no longer has.

## What the patch does

rust-windowing/winit#4723, "ios: support the UIScene lifecycle", applied
verbatim to `src/` (`app_state.rs`, `event_loop.rs`, `lib.rs`, `window.rs`),
and the two UIKit features it needs added to `Cargo.toml` (`UIScene`,
`UIWindowScene`) by hand, as the PR's manifest hunk does not apply to the
published crate. No `leaf patch` marks: the diff is the PR's.

The app declares the scene configuration (`lib.rs`'s "Scenes" says how); no
scene delegate is needed. `spikes/blitz-ios-a11y/ios/Info.plist` has one.

## When it goes

When #4723 is in a winit release that dioxus-native takes. Until then the
proposal "A shared, patched winit for the Dioxus work" says to support the PR
upstream rather than open another.
