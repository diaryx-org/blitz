use std::sync::Arc;

use anyrender::filters::{EdgeMode, Filter, FilterEffect};
use vello_common::filter_effects::FilterPrimitive;

pub(crate) fn convert_filter(filter: Arc<Filter>) -> Option<vello_common::filter_effects::Filter> {
    let nodes = filter.nodes();
    if nodes.is_empty() {
        return None;
    }

    // Vello Hybrid only supports single-node filters at the moment
    // PATCH(diaryx): every step of the filter, in order, not the first alone; vello_gpu draws a
    // first step and the color matrices after it. A step it cannot draw is skipped.
    let mut graph = vello_common::filter_effects::FilterGraph::new();
    let mut output = None;
    for node in nodes {
        if let Some(primitive) = convert_filter_effect(&node.effect) {
            output = Some(graph.add(primitive, None));
        }
    }
    graph.set_output(output?);
    Some(vello_common::filter_effects::Filter {
        graph: std::sync::Arc::new(graph),
    })
}

pub(crate) fn convert_filter_effect(effect: &FilterEffect) -> Option<FilterPrimitive> {
    Some(match effect {
        FilterEffect::Flood(color) => FilterPrimitive::Flood { color: *color },
        FilterEffect::GaussianBlur(blur) => FilterPrimitive::GaussianBlur {
            std_deviation: blur.std_deviation,
            edge_mode: convert_edge_mode(blur.edge_mode),
        },
        FilterEffect::DropShadow(shadow) => FilterPrimitive::DropShadow {
            dx: shadow.dx,
            dy: shadow.dy,
            std_deviation: shadow.std_deviation,
            color: shadow.color,
            edge_mode: convert_edge_mode(shadow.edge_mode),
        },
        FilterEffect::Offset(offset) => FilterPrimitive::Offset {
            dx: offset.x as f32,
            dy: offset.y as f32,
        },
        // PATCH(diaryx): vello_gpu draws a color matrix.
        FilterEffect::ColorMatrix(matrix) => FilterPrimitive::ColorMatrix { matrix: matrix.0 },
        FilterEffect::Blend(_mode) => return None, //FilterPrimitive::Blend { mode: *mode },
        FilterEffect::ComponentTransfer(_component_transfer_filter) => return None,
        FilterEffect::Composite(_composite_operator) => return None,
        FilterEffect::Morphology(_morphology_filter) => return None,
        FilterEffect::ConvolveMatrix(_convolution_kernel) => return None,
        FilterEffect::Turbulence(_turbulence_filter) => return None,
        FilterEffect::DisplacementMap(_displacement_map_filter) => return None,
        FilterEffect::Image(_external_image_source) => return None,
        FilterEffect::Tile => return None,
        FilterEffect::DiffuseLighting(_diffuse_lighting_filter) => return None,
        FilterEffect::SpecularLighting(_specular_lighting_filter) => return None,
    })
}

fn convert_edge_mode(edge_mode: EdgeMode) -> vello_common::filter_effects::EdgeMode {
    match edge_mode {
        EdgeMode::Duplicate => vello_common::filter_effects::EdgeMode::Duplicate,
        EdgeMode::Wrap => vello_common::filter_effects::EdgeMode::Wrap,
        EdgeMode::Mirror => vello_common::filter_effects::EdgeMode::Mirror,
        EdgeMode::None => vello_common::filter_effects::EdgeMode::None,
    }
}
