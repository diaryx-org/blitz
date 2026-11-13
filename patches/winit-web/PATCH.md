# winit-web 0.31.0-beta.3, patched

The crate dioxus-native pins, patched for Diaryx's browser build and kept
here, in diaryx-org's Blitz fork, beside the Blitz it is built with. It is
vendored unmodified in the commit before the patch. Every change to winit's
own files is marked `leaf patch`; `src/web_sys/ime.rs` is new.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`.

## What was wrong

**No input method, so no soft keyboard.** winit-web reports no IME
capabilities and refuses every `ImeRequest` (`NotSupported`). It listens for
keys on the canvas, which is not editable, so a phone or tablet never raises
its keyboard over it and a keyboard's suggestions, corrections, accents and
dictation have nowhere to go. A page has no API to raise a keyboard: one
comes up only for an editable element given focus, and on iOS only when the
focus is given while a tap is being handled.

## What the patch does

`ime.rs` gives the window an input method through a hidden `<textarea>` of
its own, marked `data-winit-ime`, laid over the IME cursor area and
invisible. Its module documentation has the detail.

- `ImeRequest::Enable` makes the textarea, emits `Ime::Enabled` and focuses
  it, which raises the keyboard. `Update` places it at the cursor area and
  gives it the surrounding text and selection, so suggestions and
  capitalisation see the text the caret is in. `Disable` gives focus back
  to the canvas and emits `Ime::Disabled`.
- What the input method does to the textarea is read back as a difference
  from what it held, and sent as `Ime::DeleteSurrounding` and `Ime::Commit`.
  A composition is committed as it goes, not shown as preedit.
- Keys that edit or move still arrive as `KeyboardInput`, from the textarea
  as from the canvas (the two share the canvas's key handlers). A keyboard
  that sends none for them (key code 229, as Android's do) has its
  `beforeinput` turned into Enter, Backspace or Delete.
- Under a finger (`pointer: coarse`), `Enable` is honoured only while a tap
  is being handled, and refused otherwise, so a tap on a button or an editor
  that asks as it mounts does not bring the keyboard up. The keyboard closed
  by the person blurs the textarea, which disables the IME, so the next tap
  on text asks again. A press on the canvas does not take focus from the
  textarea, which would drop the keyboard on every tap.
- With a real keyboard, `Enable` is always honoured, and a click on the
  canvas gives the textarea its focus back while the IME is enabled.
- Focus moving between the canvas and the textarea is not a window losing
  focus.

An application that never asks for the IME gets the old behaviour.

## What it does not do yet

- The canvas does not follow `visualViewport`, so a keyboard covers the
  bottom of the page rather than shrinking it.
- A composition is not underlined while it runs, since it is committed as it
  goes.

## Upstream

Not prepared. Upstream winit-web has no input method at all; a hidden
editable element is how browser-based editors over a canvas get one.
