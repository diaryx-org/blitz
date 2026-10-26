//! PATCH(diaryx): drawing on the CPU when there is no GPU to draw with.
//!
//! [`VelloWindowRenderer`](crate::VelloWindowRenderer) changes to this when
//! wgpu gives it no adapter, device or surface (a virtual machine, a weak or
//! missing GPU driver), so that the window still draws instead of the app
//! panicking.

use std::sync::Arc;

use anyrender::{Filter, NormalizedCoord, PaintRef, PaintScene, RenderContext, ResourceId};
use anyrender_vello_cpu::VelloCpuScenePainter;
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

use crate::VelloScenePainter;

pub(crate) use anyrender_vello_cpu::{
    VelloCpuRendererOptions as SoftwareRendererOptions, VelloCpuWindowRenderer as SoftwareRenderer,
};

/// The painter [`VelloWindowRenderer`](crate::VelloWindowRenderer) hands to
/// its draw function: Vello's on the GPU, or vello_cpu's after a fallback.
pub enum VelloWindowScenePainter<'a> {
    Gpu(VelloScenePainter<'a, 'a>),
    Cpu(&'a mut VelloCpuScenePainter),
}

macro_rules! delegate {
    ($self:ident, $painter:ident => $call:expr) => {
        match $self {
            VelloWindowScenePainter::Gpu($painter) => $call,
            VelloWindowScenePainter::Cpu($painter) => $call,
        }
    };
}

impl RenderContext for VelloWindowScenePainter<'_> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn std::any::Any>,
    ) -> Result<ResourceId, anyrender::RegisterResourceError> {
        delegate!(self, p => p.try_register_custom_resource(resource))
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        delegate!(self, p => p.unregister_resource(resource_id))
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn std::any::Any>> {
        delegate!(self, p => p.renderer_specific_context())
    }
}

impl PaintScene for VelloWindowScenePainter<'_> {
    fn reset(&mut self) {
        delegate!(self, p => p.reset())
    }

    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        delegate!(self, p => p.push_layer(fill, blend, alpha, transform, clip, filter, backdrop_filter))
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        delegate!(self, p => p.push_clip_layer(fill, transform, clip))
    }

    fn pop_layer(&mut self) {
        delegate!(self, p => p.pop_layer())
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        paint_ref: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        delegate!(self, p => p.stroke(style, transform, paint_ref, brush_transform, shape))
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        paint: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        delegate!(self, p => p.fill(style, transform, paint, brush_transform, shape))
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: kurbo::Vec2,
        style: impl Into<StyleRef<'a>>,
        paint: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph> + Clone,
    ) {
        delegate!(self, p => p.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            paint,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
        ))
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        delegate!(self, p => p.draw_box_shadow(transform, rect, brush, radius, std_dev))
    }
}
