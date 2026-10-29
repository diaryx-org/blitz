//! PATCH(diaryx): more windows than the one the app is launched in.
//!
//! [`open_window`] asks for another window, running a component in a
//! `VirtualDom` of its own. The window is made the next time the event loop
//! reads its proxy's events, with what [`launch_cfg_with_props`] made the
//! first one with: the root contexts it was given, its net provider and HTML
//! parser, the fonts, and the renderer's options. Every window's `VirtualDom`
//! runs on the event loop's thread, so state a component shares with another
//! window's can live in a `thread_local`.
//!
//! [`launch_cfg_with_props`]: crate::launch_cfg_with_props

use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;

use blitz_dom::{DocumentConfig, FontContext, HtmlParserProvider};
use blitz_shell::{BlitzShellEvent, BlitzShellProxy, WindowConfig};
use blitz_traits::net::NetProvider;
use dioxus_core::{ComponentFunction, VirtualDom};
use winit::window::WindowAttributes;

use crate::link_handler::DioxusNativeNavigationProvider;
use crate::{DioxusDocument, DioxusNativeEvent, DioxusNativeWindowRenderer, RendererOptions};

/// A root context, as `launch_cfg` takes them: made again for each window.
pub(crate) type RootContext = Box<dyn Fn() -> Box<dyn Any> + Send + Sync>;

/// What every window is made with, kept from launch.
pub(crate) struct Template {
    pub(crate) contexts: Vec<RootContext>,
    /// The net provider's inner provider, which the first window's
    /// `VirtualDom` is given as a root context; each window's is too.
    #[cfg(all(feature = "net", not(target_arch = "wasm32")))]
    pub(crate) inner_net_provider: Arc<blitz_net::Provider>,
    pub(crate) net_provider: Arc<dyn NetProvider>,
    pub(crate) html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    pub(crate) font_ctx: Option<FontContext>,
    pub(crate) renderer: RendererOptions,
}

impl Template {
    /// A window running `vdom`, with the template's root contexts given to
    /// it and a renderer of its own.
    pub(crate) fn window(
        &self,
        mut vdom: VirtualDom,
        attributes: WindowAttributes,
    ) -> WindowConfig<DioxusNativeWindowRenderer> {
        for context in &self.contexts {
            vdom.insert_any_root_context(context());
        }
        #[cfg(all(feature = "net", not(target_arch = "wasm32")))]
        vdom.provide_root_context(Arc::clone(&self.inner_net_provider));
        vdom.provide_root_context(Arc::clone(&self.net_provider));
        if let Some(html_parser) = &self.html_parser_provider {
            vdom.provide_root_context(Arc::clone(html_parser));
        }

        let doc = DioxusDocument::new(
            vdom,
            DocumentConfig {
                net_provider: Some(Arc::clone(&self.net_provider)),
                html_parser_provider: self.html_parser_provider.clone(),
                navigation_provider: Some(Arc::new(DioxusNativeNavigationProvider) as _),
                font_ctx: self.font_ctx.clone(),
                ..Default::default()
            },
        );
        let renderer = DioxusNativeWindowRenderer::with_options(RendererOptions {
            base_color: self.renderer.base_color,
            alpha_mode: self.renderer.alpha_mode,
            #[cfg(any(feature = "vello", feature = "vello-hybrid"))]
            features: self.renderer.features,
            #[cfg(any(feature = "vello", feature = "vello-hybrid"))]
            limits: self.renderer.limits.clone(),
        });
        WindowConfig::with_attributes(Box::new(doc) as _, renderer, attributes)
    }
}

/// A window asked for and not yet made.
pub(crate) struct Opening {
    pub(crate) vdom: Box<dyn FnOnce() -> VirtualDom>,
    pub(crate) attributes: WindowAttributes,
}

thread_local! {
    /// The event loop's proxy, set at launch, for [`open_window`] to wake it.
    static PROXY: RefCell<Option<BlitzShellProxy>> = const { RefCell::new(None) };
    /// The windows asked for since the event loop last made any.
    static OPENING: RefCell<Vec<Opening>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn set_proxy(proxy: BlitzShellProxy) {
    PROXY.with_borrow_mut(|p| *p = Some(proxy));
}

/// The windows asked for, oldest first, taken from the queue.
pub(crate) fn take_openings() -> Vec<Opening> {
    OPENING.with_borrow_mut(std::mem::take)
}

/// Open another window, running `app` with `props` in a `VirtualDom` of its
/// own, with `attributes`. It is made when the event loop next reads its
/// proxy's events, not before this returns; closing it ends the app only if
/// it is the last window open.
///
/// Call it on the event loop's thread (from a component, an event handler or
/// a task) after launch. Before launch, or on another thread, it does
/// nothing and returns `false`.
pub fn open_window<P: Clone + 'static, M: 'static>(
    app: impl ComponentFunction<P, M>,
    props: P,
    attributes: WindowAttributes,
) -> bool {
    let Some(proxy) = PROXY.with_borrow(|p| p.clone()) else {
        return false;
    };
    OPENING.with_borrow_mut(|o| {
        o.push(Opening {
            vdom: Box::new(move || VirtualDom::new_with_props(app, props)),
            attributes,
        })
    });
    proxy.send_event(BlitzShellEvent::embedder_event(
        DioxusNativeEvent::OpenWindows,
    ));
    true
}
