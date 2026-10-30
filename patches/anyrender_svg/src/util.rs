// Copyright 2023 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use anyrender::{Paint, PaintScene};
use kurbo::{Affine, BezPath, Point, Rect, Stroke};
use peniko::color::{self, DynamicColor};
use peniko::{Color, Fill, Mix};

#[cfg(feature = "image")]
use peniko::{Blob, ImageBrush};

pub(crate) fn to_affine(ts: &usvg::Transform) -> Affine {
    let usvg::Transform {
        sx,
        kx,
        ky,
        sy,
        tx,
        ty,
    } = ts;
    Affine::new([sx, ky, kx, sy, tx, ty].map(|&x| f64::from(x)))
}

pub(crate) fn to_stroke(stroke: &usvg::Stroke) -> Stroke {
    let mut conv_stroke = Stroke::new(stroke.width().get() as f64)
        .with_caps(match stroke.linecap() {
            usvg::LineCap::Butt => kurbo::Cap::Butt,
            usvg::LineCap::Round => kurbo::Cap::Round,
            usvg::LineCap::Square => kurbo::Cap::Square,
        })
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => kurbo::Join::Miter,
            usvg::LineJoin::Round => kurbo::Join::Round,
            usvg::LineJoin::Bevel => kurbo::Join::Bevel,
        })
        .with_miter_limit(stroke.miterlimit().get() as f64);
    if let Some(dash_array) = stroke.dasharray().as_ref() {
        conv_stroke = conv_stroke.with_dashes(
            stroke.dashoffset() as f64,
            dash_array.iter().map(|x| *x as f64),
        );
    }
    conv_stroke
}

pub(crate) fn to_mix(blend_mode: usvg::BlendMode) -> Mix {
    match blend_mode {
        usvg::BlendMode::Normal => Mix::Normal,
        usvg::BlendMode::Multiply => Mix::Multiply,
        usvg::BlendMode::Screen => Mix::Screen,
        usvg::BlendMode::Overlay => Mix::Overlay,
        usvg::BlendMode::Darken => Mix::Darken,
        usvg::BlendMode::Lighten => Mix::Lighten,
        usvg::BlendMode::ColorDodge => Mix::ColorDodge,
        usvg::BlendMode::ColorBurn => Mix::ColorBurn,
        usvg::BlendMode::HardLight => Mix::HardLight,
        usvg::BlendMode::SoftLight => Mix::SoftLight,
        usvg::BlendMode::Difference => Mix::Difference,
        usvg::BlendMode::Exclusion => Mix::Exclusion,
        usvg::BlendMode::Hue => Mix::Hue,
        usvg::BlendMode::Saturation => Mix::Saturation,
        usvg::BlendMode::Color => Mix::Color,
        usvg::BlendMode::Luminosity => Mix::Luminosity,
    }
}

pub(crate) fn to_bez_path(path: &usvg::Path) -> BezPath {
    let mut local_path = BezPath::new();
    // The semantics of SVG paths don't line up with `BezPath`; we
    // must manually track initial points
    let mut just_closed = false;
    let mut most_recent_initial = (0., 0.);
    for elt in path.data().segments() {
        match elt {
            usvg::tiny_skia_path::PathSegment::MoveTo(p) => {
                if std::mem::take(&mut just_closed) {
                    local_path.move_to(most_recent_initial);
                }
                most_recent_initial = (p.x.into(), p.y.into());
                local_path.move_to(most_recent_initial);
            }
            usvg::tiny_skia_path::PathSegment::LineTo(p) => {
                if std::mem::take(&mut just_closed) {
                    local_path.move_to(most_recent_initial);
                }
                local_path.line_to(Point::new(p.x as f64, p.y as f64));
            }
            usvg::tiny_skia_path::PathSegment::QuadTo(p1, p2) => {
                if std::mem::take(&mut just_closed) {
                    local_path.move_to(most_recent_initial);
                }
                local_path.quad_to(
                    Point::new(p1.x as f64, p1.y as f64),
                    Point::new(p2.x as f64, p2.y as f64),
                );
            }
            usvg::tiny_skia_path::PathSegment::CubicTo(p1, p2, p3) => {
                if std::mem::take(&mut just_closed) {
                    local_path.move_to(most_recent_initial);
                }
                local_path.curve_to(
                    Point::new(p1.x as f64, p1.y as f64),
                    Point::new(p2.x as f64, p2.y as f64),
                    Point::new(p3.x as f64, p3.y as f64),
                );
            }
            usvg::tiny_skia_path::PathSegment::Close => {
                just_closed = true;
                local_path.close_path();
            }
        }
    }

    local_path
}

pub(crate) fn to_brush(paint: &usvg::Paint, opacity: usvg::Opacity) -> Option<(Paint, Affine)> {
    match paint {
        usvg::Paint::Color(color) => Some((
            Paint::Solid(Color::from_rgba8(
                color.red,
                color.green,
                color.blue,
                opacity.to_u8(),
            )),
            Affine::IDENTITY,
        )),
        usvg::Paint::LinearGradient(gr) => {
            let stops: Vec<peniko::ColorStop> = gr
                .stops()
                .iter()
                .map(|stop| peniko::ColorStop {
                    offset: stop.offset().get(),
                    color: DynamicColor::from_alpha_color(Color::from_rgba8(
                        stop.color().red,
                        stop.color().green,
                        stop.color().blue,
                        (stop.opacity() * opacity).to_u8(),
                    )),
                })
                .collect();
            let start = Point::new(gr.x1() as f64, gr.y1() as f64);
            let end = Point::new(gr.x2() as f64, gr.y2() as f64);
            let arr = [
                gr.transform().sx,
                gr.transform().ky,
                gr.transform().kx,
                gr.transform().sy,
                gr.transform().tx,
                gr.transform().ty,
            ]
            .map(f64::from);
            let transform = Affine::new(arr);
            let gradient = peniko::Gradient::new_linear(start, end)
                .with_stops(stops.as_slice())
                .with_extend(to_extend(gr.spread_method()));
            Some((Paint::Gradient(gradient), transform))
        }
        usvg::Paint::RadialGradient(gr) => {
            let stops: Vec<peniko::ColorStop> = gr
                .stops()
                .iter()
                .map(|stop| peniko::ColorStop {
                    offset: stop.offset().get(),
                    color: DynamicColor::from_alpha_color(Color::from_rgba8(
                        stop.color().red,
                        stop.color().green,
                        stop.color().blue,
                        (stop.opacity() * opacity).to_u8(),
                    )),
                })
                .collect();

            let start_center = Point::new(gr.fx() as f64, gr.fy() as f64);
            let end_center = Point::new(gr.cx() as f64, gr.cy() as f64);
            let start_radius = gr.fr().get();
            let end_radius = gr.r().get();
            let arr = [
                gr.transform().sx,
                gr.transform().ky,
                gr.transform().kx,
                gr.transform().sy,
                gr.transform().tx,
                gr.transform().ty,
            ]
            .map(f64::from);
            let transform = Affine::new(arr);
            let gradient = peniko::Gradient::new_two_point_radial(
                start_center,
                start_radius,
                end_center,
                end_radius,
            )
            .with_stops(stops.as_slice())
            .with_extend(to_extend(gr.spread_method()));
            Some((Paint::Gradient(gradient), transform))
        }
        usvg::Paint::Pattern(_) => None,
    }
}

fn to_extend(spread_method: usvg::SpreadMethod) -> peniko::Extend {
    match spread_method {
        usvg::SpreadMethod::Pad => peniko::Extend::Pad,
        usvg::SpreadMethod::Reflect => peniko::Extend::Reflect,
        usvg::SpreadMethod::Repeat => peniko::Extend::Repeat,
    }
}

/// Error handler function for [`super::append_tree_with`] which draws a transparent red box
/// instead of unsupported SVG features
pub(crate) fn default_error_handler<S: PaintScene>(scene: &mut S, node: &usvg::Node) {
    let bb = node.bounding_box();
    let rect = Rect {
        x0: bb.left() as f64,
        y0: bb.top() as f64,
        x1: bb.right() as f64,
        y1: bb.bottom() as f64,
    };
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color::palette::css::RED.multiply_alpha(0.5),
        None,
        &rect,
    );
}

// PATCH(diaryx): a raster image is decoded once, not on every frame, and
// turned the way its EXIF orientation says (PATCH.md).

/// The raster image `img` draws, decoded once while its bytes are alive,
/// and the transform from its pixels to the user space it is drawn in.
///
/// usvg keeps an image's bytes in an `Arc` for as long as its tree lives,
/// and the tree is drawn again on every frame: decoded each time, a photo
/// from a phone's camera was some 48 MB of pixels decoded and uploaded
/// again per frame. Decoded once, its pixels are the same `Blob`, which the
/// renderer's image cache keeps on the GPU.
#[cfg(feature = "image")]
pub(crate) fn raster_image(img: &usvg::Image) -> Option<(ImageBrush, Affine)> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock, Weak};

    type Decoded = HashMap<usize, (Weak<Vec<u8>>, peniko::ImageData)>;
    static DECODED: OnceLock<Mutex<Decoded>> = OnceLock::new();

    let (bytes, format) = match img.kind() {
        usvg::ImageKind::JPEG(data) => (data, image::ImageFormat::Jpeg),
        usvg::ImageKind::PNG(data) => (data, image::ImageFormat::Png),
        usvg::ImageKind::GIF(data) => (data, image::ImageFormat::Gif),
        usvg::ImageKind::WEBP(data) => (data, image::ImageFormat::WebP),
        usvg::ImageKind::SVG(_) => return None,
    };
    let key = Arc::as_ptr(bytes) as usize;
    let decoded = DECODED.get_or_init(Mutex::default);
    // An entry's `Weak` holds its bytes' allocation, so the address is not
    // another's while the entry is kept; the pixels of bytes no tree holds
    // any more are let go.
    let found = decoded.lock().ok().and_then(|mut d| {
        d.retain(|_, (weak, _)| weak.strong_count() > 0);
        d.get(&key).map(|(_, data)| data.clone())
    });
    let data = match found {
        Some(data) => data,
        None => {
            let pixels = decode_raster_image(bytes, format).ok()?;
            let (width, height) = pixels.dimensions();
            let data = peniko::ImageData {
                data: Blob::new(Arc::new(pixels.into_vec())),
                format: peniko::ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width,
                height,
            };
            if let Ok(mut d) = decoded.lock() {
                d.insert(key, (Arc::downgrade(bytes), data.clone()));
            }
            data
        }
    };

    let quality = match img.rendering_mode() {
        usvg::ImageRendering::HighQuality => peniko::ImageQuality::High,
        usvg::ImageRendering::OptimizeQuality | usvg::ImageRendering::Smooth => {
            peniko::ImageQuality::Medium
        }
        usvg::ImageRendering::OptimizeSpeed
        | usvg::ImageRendering::CrispEdges
        | usvg::ImageRendering::Pixelated => peniko::ImageQuality::Low,
    };
    // usvg sized the image by the pixels its header names, before any
    // orientation: the turned pixels are fitted into that size.
    let size = img.size();
    let fit = Affine::scale_non_uniform(
        f64::from(size.width()) / f64::from(data.width),
        f64::from(size.height()) / f64::from(data.height),
    );
    Some((ImageBrush::new(data).with_quality(quality), fit))
}

/// The pixels of a raster image's `bytes`, turned and mirrored as its EXIF
/// orientation says, as a browser draws it (`image-orientation:
/// from-image`).
#[cfg(feature = "image")]
pub(crate) fn decode_raster_image(
    bytes: &[u8],
    format: image::ImageFormat,
) -> Result<image::RgbaImage, image::ImageError> {
    use image::ImageDecoder;
    // All `image::ImageFormat` variants exist even if the feature in the image crate is disabled,
    // but decoding fails with an Unsupported error if the image crate feature flag is disabled.
    // So we don't need any of our own feature handling here.
    let mut decoder =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), format).into_decoder()?;
    let orientation = decoder.orientation().ok();
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    Ok(image.into_rgba8())
}

// PATCH(diaryx): the tests of the patch.
#[cfg(all(test, feature = "image_format_jpeg"))]
mod tests {
    use super::*;

    /// A JPEG of `w` by `h`, red on its left half and blue on its right,
    /// with an EXIF orientation of `orientation`, as a phone's camera writes
    /// one held upright: its sensor's pixels, and the way up.
    fn jpeg_turned(w: u32, h: u32, orientation: u16) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    fn base64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                out.push(if i <= c.len() {
                    A[(n >> (18 - 6 * i) & 63) as usize] as char
                } else {
                    '='
                });
            }
        }
        out
    }

    /// An SVG drawing `jpeg` in a box `w` by `h`, as Typst's SVG does.
    fn svg_of(jpeg: &[u8], w: u32, h: u32) -> usvg::Tree {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><image width="{w}" height="{h}" preserveAspectRatio="none" href="data:image/jpeg;base64,{}"/></svg>"#,
            base64(jpeg)
        );
        usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap()
    }

    fn the_image(tree: &usvg::Tree) -> &usvg::Image {
        fn find(g: &usvg::Group) -> Option<&usvg::Image> {
            g.children().iter().find_map(|n| match n {
                usvg::Node::Image(img) => Some(&**img),
                usvg::Node::Group(g) => find(g),
                _ => None,
            })
        }
        find(tree.root()).expect("an image in the tree")
    }

    #[test]
    fn a_photo_is_turned_the_way_its_orientation_says() {
        // 6: the camera was turned a quarter clockwise; the picture is drawn
        // turned back, its left half on top.
        let image = decode_raster_image(&jpeg_turned(32, 16, 6), image::ImageFormat::Jpeg).unwrap();
        assert_eq!(image.dimensions(), (16, 32));
        let (top, bottom) = (image.get_pixel(8, 4), image.get_pixel(8, 28));
        assert!(
            top[0] > 200 && top[2] < 60,
            "the top is the red half: {top:?}"
        );
        assert!(
            bottom[2] > 200 && bottom[0] < 60,
            "the bottom is the blue half: {bottom:?}"
        );
    }

    #[test]
    fn turned_pixels_fill_the_box_the_tree_gave_the_image() {
        // Typst sizes the box by the turned picture: 16 by 32 for a sensor's
        // 32 by 16.
        let tree = svg_of(&jpeg_turned(32, 16, 6), 16, 32);
        let img = the_image(&tree);
        let (brush, fit) = raster_image(img).unwrap();
        assert_eq!((brush.image.width, brush.image.height), (16, 32));
        let ts = to_affine(&img.abs_transform()) * fit;
        let corner = ts * kurbo::Point::new(16.0, 32.0);
        assert!(
            (corner.x - 16.0).abs() < 1e-6 && (corner.y - 32.0).abs() < 1e-6,
            "{corner:?}"
        );
    }

    #[test]
    fn an_image_is_decoded_once_while_its_tree_lives() {
        let tree = svg_of(&jpeg_turned(32, 16, 1), 32, 16);
        let img = the_image(&tree);
        let (first, _) = raster_image(img).unwrap();
        let (again, _) = raster_image(img).unwrap();
        assert_eq!(first.image.data.id(), again.image.data.id());
        // Another tree of the same picture is decoded for itself.
        let other = svg_of(&jpeg_turned(32, 16, 1), 32, 16);
        let (theirs, _) = raster_image(the_image(&other)).unwrap();
        assert_ne!(first.image.data.id(), theirs.image.data.id());
    }
}
