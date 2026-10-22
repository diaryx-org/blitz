//! Pictures, painted with the terminal's graphics protocol over the cells
//! of the box Blitz laid out for them.
//!
//! The painter finds each `img` and records its box in cells
//! ([`crate::paint::Picture`]); it has already drawn a labelled frame there,
//! which stays when the picture cannot be read. Here each source is decoded
//! once and handed to ratatui-image, which speaks kitty, iTerm2 or sixel
//! where the terminal does and half-blocks where it does not. The pixels
//! come from, in order:
//!
//! - the host's [`Decoder`], if it has one, for sources it reads its own way;
//! - the image data Blitz loaded for the element, through the document's net
//!   provider: a raster as it is, an SVG rasterised;
//! - an SVG `data:` URL, rasterised (with the `svg` feature).
//!
//! A picture partly scrolled off the screen is cropped to its rows in view,
//! so it scrolls with the text instead of jumping whole to stay inside it.

use std::collections::HashMap;

use blitz_dom::BaseDocument;
use image::{DynamicImage, RgbaImage};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use crate::paint::{Grid, Picture};

/// A host's own way to read a picture's source: `None` to leave it to Blitz.
pub type Decoder = Box<dyn Fn(&str) -> Option<DynamicImage>>;

/// How many encoded crops to keep before starting again: each scroll step of
/// a partly visible picture is a new crop.
const PROTOCOLS_MAX: usize = 64;

/// How many pixels an SVG is drawn at per CSS pixel: a terminal's cell is
/// about twice the size of the cell the layout measures in.
#[cfg(feature = "svg")]
const SVG_SCALE: f32 = 2.0;

/// A source, and the rows and columns of its box in view: `(first, count)`.
type Crop = (String, (u16, u16), (u16, u16));

pub struct Pictures {
    picker: Picker,
    decoder: Option<Decoder>,
    /// Each source, decoded, or `None` when nothing could decode it.
    decoded: HashMap<String, Option<DynamicImage>>,
    /// Each crop of a source, encoded for the terminal: keyed by the source,
    /// and the first row and column in view and how many of each are.
    encoded: HashMap<Crop, StatefulProtocol>,
}

impl Default for Pictures {
    fn default() -> Self {
        Self::new()
    }
}

impl Pictures {
    /// Half-blocks until [`Pictures::query`] has asked the terminal.
    pub fn new() -> Self {
        Self {
            picker: Picker::halfblocks(),
            decoder: None,
            decoded: HashMap::new(),
            encoded: HashMap::new(),
        }
    }

    /// Ask the terminal what it can draw. In raw mode: the answers are
    /// escape sequences on stdin.
    pub fn query(&mut self) {
        if let Ok(picker) = Picker::from_query_stdio() {
            self.picker = picker;
        }
    }

    pub fn protocol(&self) -> ProtocolType {
        self.picker.protocol_type()
    }

    /// Read sources with `decoder` first.
    pub fn set_decoder(&mut self, decoder: Decoder) {
        self.decoder = Some(decoder);
        self.decoded.clear();
        self.encoded.clear();
    }

    /// Paint every picture in `grid` that decodes over its cells.
    pub fn paint(&mut self, f: &mut Frame, grid: &Grid, doc: &BaseDocument) {
        if self.encoded.len() > PROTOCOLS_MAX {
            self.encoded.clear();
        }
        for p in &grid.pictures {
            self.paint_one(f, grid, doc, p);
        }
    }

    /// `p`'s pixels, decoded once. A source nothing decoded is asked of the
    /// document again each frame, which may have loaded it since.
    fn image(&mut self, doc: &BaseDocument, p: &Picture) -> Option<&DynamicImage> {
        if !matches!(self.decoded.get(&p.src), Some(Some(_))) {
            let tried = self.decoded.contains_key(&p.src);
            let own = match (&self.decoder, tried) {
                (Some(d), false) => d(&p.src),
                _ => None,
            };
            let img = own
                .or_else(|| from_document(doc, p))
                .or_else(|| (!tried).then(|| from_url(&p.src)).flatten());
            if img.is_some() || !tried {
                self.decoded.insert(p.src.clone(), img);
            }
        }
        self.decoded.get(&p.src)?.as_ref()
    }

    fn paint_one(&mut self, f: &mut Frame, grid: &Grid, doc: &BaseDocument, p: &Picture) {
        let (top, bottom) = p.shown;
        let (left, right) = p.shown_cols;
        let (bottom, right) = (bottom.min(grid.rows as i32), right.min(grid.cols as i32));
        if top >= bottom || left >= right {
            return;
        }
        // The part of the box in view, as fractions of the whole box, cut
        // from the picture in the same proportions.
        let skip = (top - p.row) as u16;
        let shown = (bottom - top) as u16;
        let skip_cols = (left - p.col) as u16;
        let shown_cols = (right - left) as u16;
        let key = (p.src.clone(), (skip, shown), (skip_cols, shown_cols));
        if !self.encoded.contains_key(&key) {
            let Some(img) = self.image(doc, p) else {
                return;
            };
            let img = if (shown, shown_cols) == (p.rows, p.cols) {
                img.clone()
            } else {
                let cut = |len: u32, from: u16, n: u16, of: u16| {
                    let len = len as f32;
                    let a = (len * from as f32 / of as f32).round() as u32;
                    let b = (len * (from + n) as f32 / of as f32).round() as u32;
                    (a, (b.saturating_sub(a)).max(1))
                };
                let (y, h) = cut(img.height(), skip, shown, p.rows);
                let (x, w) = cut(img.width(), skip_cols, shown_cols, p.cols);
                img.crop_imm(x, y, w, h)
            };
            let proto = self.picker.new_resize_protocol(img);
            self.encoded.insert(key.clone(), proto);
        }
        let Some(proto) = self.encoded.get_mut(&key) else {
            return;
        };
        let rect = Rect::new(left as u16, top as u16, shown_cols, shown);
        f.render_widget(Clear, rect);
        f.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), rect, proto);
    }
}

/// The image data Blitz loaded for `p`'s element.
fn from_document(doc: &BaseDocument, p: &Picture) -> Option<DynamicImage> {
    use blitz_dom::node::ImageData;
    let data = doc.get_node(p.node)?.element_data()?.image_data()?;
    match data {
        ImageData::Raster(r) => {
            RgbaImage::from_raw(r.width, r.height, r.data.data().to_vec()).map(DynamicImage::from)
        }
        #[cfg(feature = "svg")]
        ImageData::Svg(s) => rasterize(&s.tree, SVG_SCALE),
        ImageData::None => None,
    }
}

/// An SVG `data:` URL, read without Blitz's help.
fn from_url(src: &str) -> Option<DynamicImage> {
    #[cfg(feature = "svg")]
    if let Some(rest) = src.strip_prefix("data:image/svg+xml") {
        let (head, body) = rest.split_once(',')?;
        // Base64 is left to Blitz's data URL loader.
        if head.ends_with(";base64") {
            return None;
        }
        let bytes = percent_decode(body)?;
        let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default()).ok()?;
        return rasterize(&tree, SVG_SCALE);
    }
    let _ = src;
    None
}

#[cfg(feature = "svg")]
fn rasterize(tree: &usvg::Tree, scale: f32) -> Option<DynamicImage> {
    use resvg::tiny_skia::{Pixmap, Transform};
    let size = tree.size();
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    resvg::render(
        tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia's pixels are premultiplied; an image's are not.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::from_raw(w, h, rgba).map(DynamicImage::from)
}

/// `%XX` escapes back to bytes.
#[cfg_attr(not(feature = "svg"), allow(dead_code))]
fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_decode_and_the_rest_stays() {
        assert_eq!(percent_decode("a%20b%3Cc").unwrap(), b"a b<c");
        assert_eq!(percent_decode("/x/y.png").unwrap(), b"/x/y.png");
    }

    #[cfg(feature = "svg")]
    #[test]
    fn an_svg_data_url_rasterises() {
        let url = "data:image/svg+xml;charset=utf-8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2210%22%20height%3D%225%22%3E%3C%2Fsvg%3E";
        let img = from_url(url).expect("an image");
        assert_eq!((img.width(), img.height()), (20, 10));
    }
}
