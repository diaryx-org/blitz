use anyrender::{
    RegisterResourceErrorKind, RenderContext, ResourceId, WindowHandle, WindowRenderer,
};
use debug_timer::debug_timer;
use futures_channel::oneshot;
use peniko::{Color, ImageData};
use rustc_hash::FxHashMap;
use std::future::Future;
use std::sync::Arc;
use vello::{
    AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
    Scene as VelloScene,
};
use wgpu::{
    CompositeAlphaMode, Features, Limits, PipelineCache, PresentMode, Texture, TextureFormat,
    TextureUsages,
};
use wgpu_context::{
    AlphaConversion, DeviceHandle, SurfaceRenderer, SurfaceRendererConfiguration,
    TextureConfiguration, WGPUContext,
};

use crate::software::{SoftwareRenderer, SoftwareRendererOptions, VelloWindowScenePainter};
use crate::{DEFAULT_THREADS, VelloScenePainter};

/// Drive the wgpu init future. On wasm32 we spawn it onto the JS microtask
/// queue (blocking is not allowed). On native we drive it inline with
/// `pollster::block_on` — there's no ambient async runtime to spawn onto, and
/// `on_ready` then fires before `resume` returns.
///
/// PATCH(diaryx): `on_failure` runs if the future fails; natively, inline
/// before `resume` returns, as `on_ready` would.
#[cfg(target_arch = "wasm32")]
fn spawn_init<F: Future<Output = Result<(), String>> + 'static>(
    f: F,
    _on_failure: impl FnOnce(String),
) {
    // No fallback on the web: the error is the panic's message.
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = f.await {
            panic!("{error}");
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_init<F: Future<Output = Result<(), String>>>(f: F, on_failure: impl FnOnce(String)) {
    if let Err(error) = pollster::block_on(f) {
        on_failure(error);
    }
}

struct ActiveRenderState {
    renderer: VelloRenderer,
    render_surface: SurfaceRenderer<'static>,
}

/// Result of a successful asynchronous resume; both the active state and the
/// `WGPUContext` are returned so the renderer can reclaim the context.
struct InitOutput {
    active: ActiveRenderState,
}

#[allow(clippy::large_enum_variant)]
enum RenderState {
    Suspended,
    Pending {
        receiver: oneshot::Receiver<InitOutput>,
    },
    Active(ActiveRenderState),
    /// PATCH(diaryx): wgpu gave no adapter, device or surface, so the window
    /// draws on the CPU. Kept through `suspend`, so a later `resume` does not
    /// ask wgpu again.
    Software(Box<SoftwareRenderer>),
}

#[derive(Clone)]
#[non_exhaustive]
pub struct VelloRendererOptions {
    pub features: Option<Features>,
    pub limits: Option<Limits>,
    pub base_color: Color,
    pub antialiasing_method: AaConfig,
    /// Alpha mode used when compositing the window surface.
    pub composite_alpha_mode: anyrender::CompositeAlphaMode,
    /// Cache that wgpu may reuse compiled pipelines from, avoiding shader
    /// compilation on start-up. Creating it, persisting its data and deciding
    /// when it is stale are all the caller's responsibility.
    pub pipeline_cache: Option<PipelineCache>,
    /// Maximum number of frames the presentation engine may queue ahead of the
    /// display. Each queued frame requires a window-sized swapchain surface, so
    /// lower values reduce memory usage and input latency at the cost of less
    /// buffering to absorb slow frames.
    pub desired_maximum_frame_latency: u32,
}

impl Default for VelloRendererOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl VelloRendererOptions {
    pub const fn new() -> Self {
        Self {
            features: None,
            limits: None,
            base_color: Color::WHITE,
            antialiasing_method: AaConfig::Msaa16,
            composite_alpha_mode: anyrender::CompositeAlphaMode::Auto,
            pipeline_cache: None,
            // PATCH(diaryx): 2, not 1. On Metal a latency of 1 gives the
            // layer two drawables, and acquiring one waited for the frame on
            // screen to be let go: about 22 ms a frame on an iPhone, scrolling
            // at 27 frames a second on a 60 Hz display.
            desired_maximum_frame_latency: 2,
        }
    }

    pub fn features(self, features: Features) -> Self {
        Self {
            features: Some(features),
            ..self
        }
    }

    pub fn limits(self, limits: Limits) -> Self {
        Self {
            limits: Some(limits),
            ..self
        }
    }

    pub fn base_color(self, base_color: Color) -> Self {
        Self { base_color, ..self }
    }

    pub fn antialiasing_method(self, antialiasing_method: AaConfig) -> Self {
        Self {
            antialiasing_method,
            ..self
        }
    }

    pub fn composite_alpha_mode(
        self,
        composite_alpha_mode: anyrender::types::CompositeAlphaMode,
    ) -> Self {
        Self {
            composite_alpha_mode,
            ..self
        }
    }

    pub fn pipeline_cache(self, pipeline_cache: PipelineCache) -> Self {
        Self {
            pipeline_cache: Some(pipeline_cache),
            ..self
        }
    }

    pub fn desired_maximum_frame_latency(self, desired_maximum_frame_latency: u32) -> Self {
        Self {
            desired_maximum_frame_latency,
            ..self
        }
    }
}

impl From<anyrender::RendererConfig> for VelloRendererOptions {
    fn from(config: anyrender::RendererConfig) -> Self {
        Self {
            base_color: config.base_color.unwrap_or(Color::WHITE),
            composite_alpha_mode: config.composite_alpha_mode.unwrap_or_default(),
            ..Default::default()
        }
    }
}

pub struct VelloWindowRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,

    wgpu_context: WGPUContext,
    scene: VelloScene,
    config: VelloRendererOptions,

    // Resources
    texture_handles: FxHashMap<ResourceId, ImageData>,
}

impl VelloWindowRenderer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_options(VelloRendererOptions::default())
    }

    pub fn with_options(config: impl Into<VelloRendererOptions>) -> Self {
        let config = config.into();
        Self {
            render_state: RenderState::Suspended,
            wgpu_context: build_wgpu_context(&config),
            config,
            window_handle: None,
            scene: VelloScene::new(),
            texture_handles: FxHashMap::default(),
        }
    }

    /// PATCH(diaryx): whether the window draws on the CPU, because wgpu gave
    /// no adapter, device or surface for it.
    pub fn is_software(&self) -> bool {
        matches!(self.render_state, RenderState::Software(_))
    }

    /// PATCH(diaryx): draw on the CPU from now on, saying why on stderr.
    fn fall_back_to_software(
        &mut self,
        error: String,
        window_handle: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
        on_ready: impl FnOnce() + 'static,
    ) {
        eprintln!("anyrender_vello: no GPU to draw with ({error}); drawing on the CPU instead");
        let mut software = Box::new(SoftwareRenderer::with_options(
            SoftwareRendererOptions::new().base_color(self.config.base_color),
        ));
        software.resume(window_handle, width, height, on_ready);
        self.render_state = RenderState::Software(software);
    }

    pub fn current_device_handle(&self) -> Option<&DeviceHandle> {
        match &self.render_state {
            RenderState::Active(active) => Some(&active.render_surface.device_handle),
            _ => None,
        }
    }
}

impl RenderContext for VelloWindowRenderer {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn std::any::Any>,
    ) -> Result<ResourceId, anyrender::RegisterResourceError> {
        let active = match &mut self.render_state {
            RenderState::Active(active) => active,
            RenderState::Software(software) => {
                return software.try_register_custom_resource(resource);
            }
            _ => return Err(RegisterResourceErrorKind::NotActive.into()),
        };

        if let Ok(texture) = resource.downcast::<Texture>() {
            let id = ResourceId::new();
            self.texture_handles
                .insert(id, active.renderer.register_texture(*texture));
            Ok(id)
        } else {
            Err(anyrender::RegisterResourceErrorKind::UnsupportedResourceKind.into())
        }
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        let active = match &mut self.render_state {
            RenderState::Active(active) => active,
            RenderState::Software(software) => return software.unregister_resource(resource_id),
            _ => return,
        };

        if let Some(handle) = self.texture_handles.remove(&resource_id) {
            active.renderer.unregister_texture(handle);
        }
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn std::any::Any>> {
        match &self.render_state {
            RenderState::Active(active) => {
                Some(Box::new(active.render_surface.device_handle.clone()))
            }
            RenderState::Software(software) => software.renderer_specific_context(),
            RenderState::Pending { .. } => None,
            RenderState::Suspended => None,
        }
    }
}

fn build_wgpu_context(config: &VelloRendererOptions) -> WGPUContext {
    let features =
        config.features.unwrap_or_default() | Features::CLEAR_TEXTURE | Features::PIPELINE_CACHE;
    WGPUContext::with_features_and_limits(Some(features), config.limits.clone())
}

impl WindowRenderer for VelloWindowRenderer {
    type ScenePainter<'a>
        = VelloWindowScenePainter<'a>
    where
        Self: 'a;

    fn is_active(&self) -> bool {
        match &self.render_state {
            RenderState::Active { .. } => true,
            RenderState::Software(software) => software.is_active(),
            _ => false,
        }
    }

    fn is_pending(&self) -> bool {
        matches!(self.render_state, RenderState::Pending { .. })
    }

    fn resume<F: FnOnce() + 'static>(
        &mut self,
        window_handle: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
        on_ready: F,
    ) {
        // Each `resume` must be preceded by `suspend` (or be the first call after
        // construction). Calling while `Pending` or `Active` is a state-machine bug
        // in the embedder: it would orphan the in-flight init's `WGPUContext` and
        // pay for a fresh adapter+device init on the fallback path below.
        if let RenderState::Software(software) = &mut self.render_state
            && !software.is_active()
        {
            self.window_handle = Some(window_handle.clone());
            software.resume(window_handle, width, height, on_ready);
            return;
        }
        if !matches!(self.render_state, RenderState::Suspended) {
            // #[cfg(feature = "tracing")]
            // tracing::warn!("WindowRenderer::resume called from non-Suspended state");
            return;
        }

        let (sender, receiver) = oneshot::channel();
        self.render_state = RenderState::Pending { receiver };
        self.window_handle = Some(window_handle.clone());

        // PATCH(diaryx): no surface, adapter or device falls back to the CPU
        // (`fall_back_to_software`), where it used to panic.
        let surface = match self.wgpu_context.create_surface(window_handle.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                let error = format!("Error creating surface: {error}");
                self.fall_back_to_software(error, window_handle, width, height, on_ready);
                return;
            }
        };
        let instance = self.wgpu_context.instance.clone();
        let extra_features = self.wgpu_context.extra_features();
        let override_limits = self.wgpu_context.override_limits();
        let mut composite_alpha_mode = match self.config.composite_alpha_mode {
            anyrender::CompositeAlphaMode::Auto => CompositeAlphaMode::Auto,
            anyrender::CompositeAlphaMode::Opaque => CompositeAlphaMode::Opaque,
            anyrender::CompositeAlphaMode::Transparent => {
                #[cfg(target_vendor = "apple")]
                {
                    // wgpu is lying in apple's case it uses PreMultiplied in reality
                    // (do not modify shaders for PostMultiplied)
                    CompositeAlphaMode::PostMultiplied
                }
                #[cfg(not(target_vendor = "apple"))]
                {
                    CompositeAlphaMode::PreMultiplied
                }
            }
        };
        let pipeline_cache = self.config.pipeline_cache.clone();
        let desired_maximum_frame_latency = self.config.desired_maximum_frame_latency;
        let existing_device_handle = self
            .wgpu_context
            .find_compatible_device_handle(Some(&surface));

        // Natively the init runs inline, so a failure falls back before
        // `resume` returns; the `on_ready` it has not used goes with it.
        let on_ready = std::rc::Rc::new(std::cell::Cell::new(Some(on_ready)));
        let on_ready_for_init = on_ready.clone();
        let init = async move {
            let device_handle = match existing_device_handle {
                Some(device_handle) => device_handle,
                None => DeviceHandle::new_from_compatible_surface(
                    instance,
                    Some(&surface),
                    extra_features,
                    override_limits,
                )
                .await
                .map_err(|error| format!("Error creating DeviceHandle: {error}"))?,
            };

            let adapter = &device_handle.adapter;
            let caps = surface.get_capabilities(adapter);
            let mut alpha_modes = caps.alpha_modes;

            if !alpha_modes.contains(&composite_alpha_mode) {
                alpha_modes.sort_unstable_by(
                    |first: &CompositeAlphaMode, second: &CompositeAlphaMode| {
                        let first_num = match *first {
                            CompositeAlphaMode::PreMultiplied => 0,
                            CompositeAlphaMode::PostMultiplied => 1,
                            CompositeAlphaMode::Opaque
                            | CompositeAlphaMode::Inherit
                            | CompositeAlphaMode::Auto => 2,
                        };
                        let second_num = match *second {
                            CompositeAlphaMode::PreMultiplied => 0,
                            CompositeAlphaMode::PostMultiplied => 1,
                            CompositeAlphaMode::Opaque
                            | CompositeAlphaMode::Inherit
                            | CompositeAlphaMode::Auto => 2,
                        };
                        first_num.cmp(&second_num)
                    },
                );
                composite_alpha_mode = alpha_modes
                    .first()
                    .copied()
                    .ok_or("Surface didn't report any alpha modes")?;
            }

            #[cfg(not(target_vendor = "apple"))]
            let texture_config = Some(TextureConfiguration {
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                format: TextureFormat::Rgba8Unorm,
                alpha_conversion: (composite_alpha_mode == CompositeAlphaMode::PreMultiplied)
                    .then_some(AlphaConversion::Premultiply),
            });
            // TODO: Remove below once gfx-rs/wgpu#9896 gets fixed
            #[cfg(target_vendor = "apple")]
            let texture_config = Some(TextureConfiguration {
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                format: TextureFormat::Rgba8Unorm,
                alpha_conversion: (composite_alpha_mode == CompositeAlphaMode::PostMultiplied
                    || composite_alpha_mode == CompositeAlphaMode::PreMultiplied)
                    .then_some(AlphaConversion::Premultiply),
            });

            let render_surface = SurfaceRenderer::new(
                surface,
                SurfaceRendererConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    formats: vec![TextureFormat::Rgba8Unorm, TextureFormat::Bgra8Unorm],
                    width,
                    height,
                    present_mode: PresentMode::AutoVsync,
                    desired_maximum_frame_latency,
                    alpha_mode: composite_alpha_mode,
                    view_formats: vec![],
                },
                texture_config,
                device_handle,
            )
            .map_err(|error| format!("Error creating SurfaceRenderer: {error}"))?;

            let renderer = VelloRenderer::new(
                render_surface.device(),
                RendererOptions {
                    antialiasing_support: AaSupport::all(),
                    use_cpu: false,
                    num_init_threads: DEFAULT_THREADS,
                    pipeline_cache,
                },
            )
            .map_err(|error| format!("Error creating the Vello renderer: {error}"))?;

            let _ = sender.send(InitOutput {
                active: ActiveRenderState {
                    renderer,
                    render_surface,
                },
            });
            if let Some(on_ready) = on_ready_for_init.take() {
                on_ready();
            }
            Ok(())
        };
        let mut failure = None;
        spawn_init(init, |error| failure = Some(error));

        if let Some(error) = failure
            && let Some(on_ready) = on_ready.take()
        {
            self.fall_back_to_software(error, window_handle, width, height, on_ready);
        }
    }

    fn complete_resume(&mut self) -> bool {
        match &mut self.render_state {
            RenderState::Active { .. } => true,
            RenderState::Software(software) => software.complete_resume(),
            RenderState::Suspended => false,
            RenderState::Pending { receiver } => match receiver.try_recv() {
                Ok(Some(InitOutput { active })) => {
                    let device_handle = active.render_surface.device_handle.clone();
                    self.wgpu_context.device_pool.push(device_handle);
                    self.render_state = RenderState::Active(active);
                    true
                }
                _ => false,
            },
        }
    }

    fn suspend(&mut self) {
        if let RenderState::Software(software) = &mut self.render_state {
            software.suspend();
            return;
        }
        if let RenderState::Active(active) = &mut self.render_state {
            // Unregister all textures on suspend
            for (_id, handle) in self.texture_handles.drain() {
                active.renderer.unregister_texture(handle);
            }
        }
        self.render_state = RenderState::Suspended;
    }

    fn set_size(&mut self, width: u32, height: u32) {
        match &mut self.render_state {
            RenderState::Active(active) => active.render_surface.resize(width, height),
            RenderState::Software(software) => software.set_size(width, height),
            _ => {}
        }
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        let state = match &mut self.render_state {
            RenderState::Active(state) => state,
            RenderState::Software(software) => {
                return software
                    .render(|painter| draw_fn(&mut VelloWindowScenePainter::Cpu(painter)));
            }
            _ => return,
        };

        let render_surface = &mut state.render_surface;

        debug_timer!(timer, feature = "log_frame_times");

        // Regenerate the vello scene
        draw_fn(&mut VelloWindowScenePainter::Gpu(VelloScenePainter {
            inner: &mut self.scene,
            renderer: Some(&mut state.renderer),
            device_handle: Some(&render_surface.device_handle),
            texture_handles: Some(&mut self.texture_handles),
        }));
        timer.record_time("cmd");

        let Ok(texture_view) = render_surface.target_texture_view() else {
            // Skip frame in case of error trying to get current surface texture
            render_surface.clear_surface_texture();
            self.scene.reset();
            return;
        };

        for handle in self.texture_handles.values() {
            state.renderer.mark_override_image_dirty(handle);
        }

        state
            .renderer
            .render_to_texture(
                render_surface.device(),
                render_surface.queue(),
                &self.scene,
                &texture_view,
                &RenderParams {
                    base_color: self.config.base_color,
                    width: render_surface.config.width,
                    height: render_surface.config.height,
                    antialiasing_method: self.config.antialiasing_method,
                },
            )
            .expect("failed to render to texture");
        timer.record_time("render");

        drop(texture_view);

        if render_surface.maybe_blit_and_present().is_err() {
            self.scene.reset();
            return;
        }
        timer.record_time("present");

        render_surface
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();

        timer.record_time("wait");
        timer.print_times("vello: ");

        // Empty the Vello scene (memory optimisation)
        self.scene.reset();
    }
}
