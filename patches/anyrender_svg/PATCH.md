# anyrender_svg 0.15.0, patched

The crate blitz-paint draws an SVG `<img>` with, kept here beside the Blitz
that calls it. Vendored from crates.io on 2026-10-29; `git log -p --
patches/anyrender_svg` here is the patch.

Consumers take it with `[patch.crates-io]` in their root manifest, from this
repository's branch the rest of their Blitz comes from, `{ git =
"https://github.com/diaryx-org/blitz", branch = "leaf" }`. Its manifest is
the published one, which takes anyrender and usvg from crates.io, so this
crate alone is patched.

## What was wrong

leaf's Reflow and Pages views (`leaf-pages`) draw a document as Typst sets
it, each piece of a page an SVG, and a picture in the document is a JPEG or
PNG inside its piece's SVG. Two things went wrong with a photo from an
iPhone's camera, found in Diaryx's app on an iPhone 15 Pro (2026-10-28):

- **Every frame decoded it again.** `render_group` decoded an `<image>`'s
  bytes each time the tree was drawn, which Blitz does on every frame: a
  12-megapixel photo was some 48 MB of pixels decoded, put in a new `Blob`,
  and uploaded to Vello's image atlas again, frame after frame. The page
  lagged badly, and with the atlas full of the copies, the picture blinked
  out of the page as others were evicted.
- **It lay on its side.** A camera writes its sensor's pixels and the way up
  in the EXIF orientation. A browser turns the picture as it says
  (`image-orientation: from-image`); this crate drew the raw pixels. Typst
  sizes the picture's box by the turned picture, so a portrait photo drew
  on its side and stretched into a portrait box.

## What the patch does

`util::raster_image` decodes an image once while the tree holds its bytes,
keyed by the bytes' `Arc` (usvg keeps them in one for the life of the tree),
and keeps the pixels as one `Blob`, which Vello's image cache keeps on the
GPU from frame to frame. The pixels of bytes no tree holds any more are let
go the next time an image is drawn.

The pixels are turned and mirrored as the image's EXIF orientation says,
and fitted into the size usvg gave the image, which it read from the
header before any orientation. `src/util.rs` and `src/render.rs`, marked
`PATCH(diaryx)`; the tests are in `src/util.rs`:

    cargo test --manifest-path patches/anyrender_svg/Cargo.toml

Neither is upstream yet.
