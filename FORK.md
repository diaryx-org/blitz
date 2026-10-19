# diaryx-org/blitz

A fork of [DioxusLabs/blitz](https://github.com/DioxusLabs/blitz), owned by
diaryx-org for [leaf](https://github.com/diaryx-org/leaf)'s Dioxus app and
whatever else the org builds on Blitz. Upstream does what it will; this fork
carries what we need now.

## Branches

- **`leaf`**: the branch we build against. It starts at the release tag
  dioxus-native pins exactly (`v0.3.0-beta.4` for dioxus-native
  0.8.0-alpha.3), so its crates keep that version and can replace the
  published ones through `[patch.crates-io]`; our changes are commits on top.
- **`main`**: upstream's, kept as it is.

## Using it

```toml
[patch.crates-io]
blitz-dom = { git = "https://github.com/diaryx-org/blitz", branch = "leaf" }
```

Patch only the crates the branch changes; the rest stay crates.io's.

## Keeping it

- **An upstream fix** we want before its release is cherry-picked with `-x`,
  its author kept, and a note in its message of what the backport changed.
- **A new Blitz release** that dioxus-native moves to: `leaf` is rebased onto
  its tag. A change upstream now has is dropped then.
- Commits follow the diaryx-org rules: Conventional Commits, a
  `Behavioural-change:` trailer where a caller would notice.

## What is on `leaf`

Accessibility, found in leaf's Mac app and an iOS spike
(`spikes/blitz-ios-a11y` in leaf), where with these VoiceOver and agents read
and press a page:

- Bounds on every AccessKit node (DioxusLabs/blitz#842, cherry-picked), and the
  viewport's for the document node.
- A page's text as static text (a `Label`, its text the value); elements
  with no semantics as `GenericContainer`.
- The click action on buttons, links, menu items and choices.
