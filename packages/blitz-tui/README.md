# blitz-tui

Paint a Blitz document into a terminal's character cells.

Blitz builds the DOM, resolves the styles (Stylo) and lays the page out
(taffy, parley) exactly as it does for a window. blitz-tui replaces only the
step blitz-paint does for pixels: it walks the laid-out tree and writes
cells. A Dioxus app, an HTML page, or anything else that implements
`blitz_dom::Document` runs in a terminal unchanged.

```rust
let mut doc = DioxusDocument::new(VirtualDom::new(app), DocumentConfig::default());
doc.initial_build();
blitz_tui::run(&mut doc, blitz_tui::Options::default())?;
```

`examples/tui_counter` is a Dioxus counter run this way:
`cargo run -p tui_counter`.

## How a page becomes cells

- A cell is one advance of a monospace face wide and one line high
  (`Metrics`). `Options::default` adds a user-agent stylesheet that sets
  every element in that face at that size, so the page's own layout
  (wrapping, hit-testing, a caret) counts in cells without knowing it.
- Text is read off each inline root's parley layout: a line starts in the
  cell its left edge rounds to, and each cluster takes the next cell (two for
  a wide one).
- A background fills the cells whose centres it covers. A box thinner than
  half a row but a cell wide (a rule, a separator) is a line of `─`.
- A border is box-drawing characters, drawn after the children. A
  horizontal side takes the row boundary nearest it, and only a row with no
  text between the sides.
- An `img` is a framed placeholder, and then the picture itself through the
  terminal's graphics protocol (kitty, iTerm2 or sixel through ratatui-image,
  half-blocks elsewhere). The pixels come from the image data Blitz loaded,
  an SVG `data:` URL, or a host's own decoder. An opaque box painted later
  (a menu over the picture) cuts the picture back to the largest part left
  uncovered.
- An app's own caret box (a thin bar) is named in
  `PaintOptions::cursor_classes` and becomes the terminal's cursor.

## Input

`input::key` turns a terminal key into Blitz's `BlitzKeyEvent`, with its
text for a typed character, so a focused element's handlers get it. ⌃ stays
CONTROL: a terminal never sees ⌘, and a host whose shortcuts are ⌘ on macOS
maps ⌃ itself. `input::mouse` turns presses, drags, moves and the wheel
into pointer and wheel events at the cell's left part.

A host that routes keys its own way (an editor that takes them before the
DOM) writes its own loop from `paint`, `Terminal` and `input`. leaf's
terminal editor does this.

## Not done yet

- The text and caret of `<input>` and `<textarea>`, which Blitz lays out
  apart from the page's inline text.
- List markers drawn by `::marker`, outlines, shadows, gradients, transforms
  and scrollbars.
- Partial transparency beyond blending a colour over the cell under it.
- The `winit` feature's `TermWindow`, for an app that asks its window for
  size and theme, is all of winit's `Window` a terminal can answer.
