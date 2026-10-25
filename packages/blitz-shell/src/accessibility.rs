use crate::{BlitzShellEvent, event::BlitzShellProxy};
use accesskit::{ActionRequest, Rect, TreeUpdate};
use accesskit_xplat::{Adapter, EventHandler, WindowEvent as AccessKitEvent};
use blitz_dom::BaseDocument;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(not(target_os = "android"))]
use winit::raw_window_handle::HasWindowHandle;
use winit::{
    event::WindowEvent,
    window::{Window, WindowId},
};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    // /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    adapter: Adapter,
    window_id: WindowId,
    /// The embedder's changes to each tree built, made before the adapter
    /// sees it ([`crate::View::set_accessibility_tree_hook`]).
    hook: Option<TreeHook>,
}

/// What an embedder changes in the tree Blitz builds from a document, before
/// assistive tech sees it: a node the page cannot describe in markup, such as
/// an editor's text, with its runs, its selection and its value.
pub type TreeHook = Box<dyn FnMut(&BaseDocument, &mut TreeUpdate)>;

/// What an embedder carries out itself of what assistive tech asks, before
/// Blitz does: whether it did.
pub type ActionHook = Box<dyn FnMut(&ActionRequest) -> bool>;

/// The hooks set for a window by code that does not hold its
/// [`crate::View`]: a component inside a document that a launcher runs
/// (dioxus-native's), which knows only its window's id.
struct WindowHooks {
    tree: TreeHook,
    action: ActionHook,
}

thread_local! {
    static WINDOW_HOOKS: RefCell<HashMap<WindowId, WindowHooks>> = RefCell::new(HashMap::new());
}

/// Sets, for window `window`, a hook every accessibility tree built from now
/// on passes through after the [`crate::View`]'s own, and a hook that is
/// offered every action assistive tech asks before Blitz carries it out
/// (returning whether it did). For code that has the window's id but not its
/// `View`; replaces the hooks set before for the window. Call on the thread
/// that runs the event loop, as a component does.
pub fn set_accessibility_hooks(
    window: WindowId,
    tree: impl FnMut(&BaseDocument, &mut TreeUpdate) + 'static,
    action: impl FnMut(&ActionRequest) -> bool + 'static,
) {
    WINDOW_HOOKS.with_borrow_mut(|hooks| {
        hooks.insert(
            window,
            WindowHooks {
                tree: Box::new(tree),
                action: Box::new(action),
            },
        )
    });
}

/// Removes the hooks [`set_accessibility_hooks`] set for `window`. Done
/// for a window when it closes.
pub fn clear_accessibility_hooks(window: WindowId) {
    // Taken out before it drops: a hook's captures may call back in here.
    // Nothing to clear once the thread's registry is gone, at its exit.
    let gone = WINDOW_HOOKS
        .try_with(|hooks| hooks.borrow_mut().remove(&window))
        .ok()
        .flatten();
    drop(gone);
}

/// Offers `request` to the action hook set for `window`: whether it carried
/// it out. The hook runs with the registry free, so it may set or clear
/// hooks.
pub(crate) fn window_action(window: WindowId, request: &ActionRequest) -> bool {
    let Some(mut hooks) = WINDOW_HOOKS.with_borrow_mut(|hooks| hooks.remove(&window)) else {
        return false;
    };
    let done = (hooks.action)(request);
    WINDOW_HOOKS.with_borrow_mut(|all| {
        all.entry(window).or_insert(hooks);
    });
    done
}

/// Passes `tree` through the tree hook set for `window`, as
/// [`window_action`] runs its action hook.
fn window_tree(window: WindowId, doc: &BaseDocument, tree: &mut TreeUpdate) {
    let Some(mut hooks) = WINDOW_HOOKS.with_borrow_mut(|hooks| hooks.remove(&window)) else {
        return;
    };
    (hooks.tree)(doc, tree);
    WINDOW_HOOKS.with_borrow_mut(|all| {
        all.entry(window).or_insert(hooks);
    });
}

struct Handler {
    window_id: WindowId,
    proxy: BlitzShellProxy,
}
impl EventHandler for Handler {
    fn handle_accesskit_event(&self, event: AccessKitEvent) {
        self.proxy.send_event(BlitzShellEvent::Accessibility {
            window_id: self.window_id,
            data: Arc::new(event),
        });
    }
}

impl AccessibilityState {
    pub fn new(window: &dyn Window, proxy: BlitzShellProxy) -> Self {
        let window_id = window.id();
        Self {
            adapter: Adapter::with_combined_handler(
                #[cfg(target_os = "android")]
                &crate::current_android_app(),
                #[cfg(not(target_os = "android"))]
                window.window_handle().unwrap().as_raw(),
                Arc::new(Handler { window_id, proxy }),
            ),
            window_id,
            hook: None,
        }
    }

    /// Sets the hook every tree built from now on passes through.
    pub fn set_tree_hook(&mut self, hook: Option<TreeHook>) {
        self.hook = hook;
    }

    pub fn update_tree(&mut self, doc: &BaseDocument) {
        let hook = &mut self.hook;
        let window_id = self.window_id;
        self.adapter.update_if_active(|| {
            let mut tree = doc.build_accessibility_tree();
            if let Some(hook) = hook {
                hook(doc, &mut tree);
            }
            window_tree(window_id, doc, &mut tree);
            tree
        });
    }

    /// Allows reacting to window events.
    ///
    /// This must be called whenever a new window event is received
    /// and before it is handled by the application.
    pub fn process_window_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(is_focused) => {
                self.adapter.set_focus(*is_focused);
            }
            WindowEvent::Moved(_) | WindowEvent::SurfaceResized(_) => {
                let outer_position: (_, _) = window
                    .outer_position()
                    .unwrap_or_default()
                    .cast::<f64>()
                    .into();
                let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
                let inner_position: (_, _) = window.surface_position().cast::<f64>().into();
                let inner_size: (_, _) = window.surface_size().cast::<f64>().into();

                self.adapter.set_window_bounds(
                    Rect::from_origin_size(outer_position, outer_size),
                    Rect::from_origin_size(inner_position, inner_size),
                )
            }
            _ => (),
        }
    }
}

impl Drop for AccessibilityState {
    /// The window is gone: so are the hooks set for it.
    fn drop(&mut self) {
        clear_accessibility_hooks(self.window_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Action, NodeId, TreeId};
    use std::rc::Rc;

    fn press(node: u64) -> ActionRequest {
        ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(node),
            data: None,
        }
    }

    #[test]
    fn a_window_s_action_hook_is_offered_its_requests_until_cleared() {
        let window = WindowId::from_raw(7);
        let other = WindowId::from_raw(8);
        let seen = Rc::new(RefCell::new(Vec::new()));
        set_accessibility_hooks(window, |_, _| {}, {
            let seen = seen.clone();
            move |r| {
                seen.borrow_mut().push(r.target_node.0);
                r.target_node.0 == 1
            }
        });
        assert!(window_action(window, &press(1)));
        assert!(!window_action(window, &press(2)));
        assert!(!window_action(other, &press(1)));
        assert_eq!(*seen.borrow(), [1, 2]);
        clear_accessibility_hooks(window);
        assert!(!window_action(window, &press(1)));
        assert_eq!(seen.borrow().len(), 2);
    }

    #[test]
    fn a_hook_may_replace_the_hooks_while_it_runs() {
        let window = WindowId::from_raw(9);
        set_accessibility_hooks(
            window,
            |_, _| {},
            move |_| {
                set_accessibility_hooks(window, |_, _| {}, |_| false);
                true
            },
        );
        assert!(window_action(window, &press(1)));
        // The replacement it set stands.
        assert!(!window_action(window, &press(1)));
        clear_accessibility_hooks(window);
    }
}
