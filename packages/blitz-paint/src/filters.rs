use blitz_dom::util::ToColorColor as _;
use blitz_dom::{BaseDocument, LocalName};
use style::Atom;
use style::color::AbsoluteColor;
pub(crate) use style::computed_values::filter::single_value::T as StyloFilter;
use style::properties::ComputedValues;

use anyrender::filters::color_transformation::ColorMatrix;
use anyrender::filters::{Filter, FilterEffect};

/// A `filter` or `backdrop-filter`, then the SVG filter its Blitz custom property names.
///
/// Stylo's Servo build parses no `url()` in either property, so a page names an SVG `<filter>`
/// for Blitz with a custom property beside it, which a browser ignores:
///
/// ```css
/// filter: url(#goo);         /* a browser's */
/// --blitz-filter: url(#goo); /* Blitz's; --blitz-backdrop-filter for backdrop-filter */
/// ```
///
/// Blitz drops the `filter` it cannot parse and draws the `<filter>` the custom property names,
/// after the property's own functions if it has any. A custom property is inherited, so one
/// the parent holds the same is taken as inherited, not set, and not drawn again.
pub(crate) fn convert_filters(
    filters: &[StyloFilter],
    styles: &ComputedValues,
    parent_styles: Option<&ComputedValues>,
    custom_property: &str,
    dom: &BaseDocument,
) -> Option<Filter> {
    let mut effects: Vec<FilterEffect> = filters.iter().filter_map(convert_single_filter).collect();
    let id = custom_filter_id(styles, custom_property);
    let inherited = id.is_some()
        && parent_styles.is_some_and(|parent| custom_filter_id(parent, custom_property) == id);
    if let Some(id) = id.filter(|_| !inherited) {
        effects.extend(svg_filter_effects(&id, dom));
    }

    (!effects.is_empty()).then(|| Filter::linear_list(effects.into_iter()))
}

/// The id a `url(#id)` in the custom property names, if it holds one.
fn custom_filter_id(styles: &ComputedValues, custom_property: &str) -> Option<String> {
    let value = styles
        .custom_properties()
        .get_for_cssom(&Atom::from(custom_property))?
        .to_variable_value();
    let url = value
        .css
        .trim()
        .strip_prefix("url(")?
        .strip_suffix(')')?
        .trim();
    let url = url.trim_matches(|c| c == '"' || c == '\'');
    Some(url.strip_prefix('#')?.to_owned())
}

pub(crate) fn convert_single_filter(filter: &StyloFilter) -> Option<FilterEffect> {
    Some(match filter {
        StyloFilter::Blur(radius) => FilterEffect::blur(radius.px()),
        StyloFilter::Brightness(amount) => FilterEffect::brightness(amount.0),
        StyloFilter::Contrast(amount) => FilterEffect::contrast(amount.0),
        StyloFilter::Grayscale(amount) => FilterEffect::grayscale(amount.0),
        StyloFilter::HueRotate(angle) => FilterEffect::hue_rotate(angle.radians()),
        StyloFilter::Invert(amount) => FilterEffect::invert(amount.0),
        StyloFilter::Opacity(amount) => FilterEffect::opacity(amount.0),
        StyloFilter::Saturate(amount) => FilterEffect::saturate(amount.0),
        StyloFilter::Sepia(amount) => FilterEffect::sepia(amount.0),
        StyloFilter::DropShadow(shadow) => FilterEffect::drop_shadow(
            shadow.horizontal.px(),
            shadow.vertical.px(),
            shadow.blur.px(),
            // TODO: pass in correct currentColor
            shadow
                .color
                .resolve_to_absolute(&AbsoluteColor::BLACK)
                .as_color_color(),
        ),
        // Never parsed by Stylo's Servo build; see `convert_filters`.
        StyloFilter::Url(_) => return None,
    })
}

/// The steps of the SVG `<filter>` with this id in the document.
///
/// Its primitives are read in order as one chain from the source graphic, each taking the one
/// before it: `feGaussianBlur`, `feColorMatrix` (`matrix`, `saturate`, `hueRotate`) and
/// `feOffset`. That covers a blur sharpened by an alpha matrix, which merges blurred shapes
/// into one (the "gooey" effect). `in`, `result`, the filter region and `primitiveUnits` are
/// not read: lengths are taken as CSS pixels, as `userSpaceOnUse` gives them. A primitive of
/// any other kind ends the chain; an id that names no `<filter>` gives none.
fn svg_filter_effects(id: &str, dom: &BaseDocument) -> Vec<FilterEffect> {
    let mut effects = Vec::new();

    let Some(filter) = dom
        .get_element_by_id(id)
        .and_then(|node_id| dom.get_node(node_id))
        .filter(|node| {
            node.element_data()
                .is_some_and(|element| &*element.name.local == "filter")
        })
    else {
        return effects;
    };

    for &child_id in &filter.children {
        let Some(element) = dom.get_node(child_id).and_then(|node| node.element_data()) else {
            continue;
        };
        let attr = |name: &str| element.attr(LocalName::from(name));
        let numbers = |name: &str| -> Vec<f32> {
            attr(name)
                .map(|value| {
                    value
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|part| !part.is_empty())
                        .filter_map(|part| part.parse().ok())
                        .collect()
                })
                .unwrap_or_default()
        };

        let effect = match &*element.name.local {
            "feGaussianBlur" => {
                // One deviation for both axes; of two, their mean, as anyrender's blur takes one.
                match numbers("stdDeviation").as_slice() {
                    [both] => FilterEffect::blur(*both),
                    [x, y] => FilterEffect::blur((x + y) / 2.0),
                    _ => break,
                }
            }
            "feColorMatrix" => match attr("type").unwrap_or("matrix") {
                "matrix" => {
                    let Ok(matrix) = <[f32; 20]>::try_from(numbers("values")) else {
                        break;
                    };
                    FilterEffect::ColorMatrix(ColorMatrix(matrix))
                }
                "saturate" => {
                    FilterEffect::saturate(numbers("values").first().copied().unwrap_or(1.0))
                }
                "hueRotate" => FilterEffect::hue_rotate(
                    numbers("values")
                        .first()
                        .copied()
                        .unwrap_or(0.0)
                        .to_radians(),
                ),
                _ => break,
            },
            "feOffset" => {
                let dx = numbers("dx").first().copied().unwrap_or(0.0);
                let dy = numbers("dy").first().copied().unwrap_or(0.0);
                FilterEffect::Offset(kurbo::Vec2::new(dx.into(), dy.into()))
            }
            _ => break,
        };
        effects.push(effect);
    }

    effects
}
