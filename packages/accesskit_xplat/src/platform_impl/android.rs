// Copyright 2025 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

//! Android: accesskit_android's `InjectingAdapter`, on a host view.
//!
//! The host is the view the embedder names with [`set_host_view`], or else
//! GameActivity's surface view. A NativeActivity has no view of its own to
//! offer (its content view is a private class), so an app built on one adds
//! a view that covers the window and names it here. With neither, there is
//! no adapter until a host is named; then the next update makes it.

use std::sync::Mutex;

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
use accesskit_android::{
    InjectingAdapter,
    jni::{
        JNIEnv, JavaVM,
        objects::{GlobalRef, JObject},
    },
};
use android_activity::AndroidApp;

/// The view the embedder named to host the tree.
static HOST: Mutex<Option<GlobalRef>> = Mutex::new(None);

/// Names the view that hosts the accessibility tree: one that covers the
/// window, has no accessibility delegate of its own and stays for the life
/// of the activity. An adapter made from now on injects itself into it, and
/// one made before it is named does so at its next update.
pub fn set_host_view(view: GlobalRef) {
    *HOST.lock().unwrap() = Some(view);
}

type Make = Box<dyn FnOnce(&mut JNIEnv, &JObject) -> InjectingAdapter + Send>;

pub struct Adapter {
    vm: JavaVM,
    activity: GlobalRef,
    adapter: Option<InjectingAdapter>,
    /// The handlers, until a host is found for them.
    make: Option<Make>,
}

impl Adapter {
    pub fn new(
        android_app: &AndroidApp,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        // SAFETY: android-activity's VM and activity live as long as the
        // process.
        let vm = unsafe { JavaVM::from_raw(android_app.vm_as_ptr() as *mut _) }.unwrap();
        let activity = {
            let env = vm.attach_current_thread().unwrap();
            let activity = unsafe { JObject::from_raw(android_app.activity_as_ptr() as *mut _) };
            env.new_global_ref(activity).unwrap()
        };
        let make: Make = Box::new(move |env, host| {
            InjectingAdapter::new(env, host, activation_handler, action_handler)
        });
        let mut adapter = Self {
            vm,
            activity,
            adapter: None,
            make: Some(make),
        };
        adapter.inject();
        adapter
    }

    /// Makes the adapter on the host, if there is one yet.
    fn inject(&mut self) {
        if self.make.is_none() {
            return;
        }
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return;
        };
        let named = HOST.lock().unwrap().clone();
        let host = match named {
            Some(view) => env.new_local_ref(view.as_obj()).ok(),
            None => game_activity_view(&mut env, self.activity.as_obj()),
        };
        let Some(host) = host.filter(|h| !h.is_null()) else {
            return;
        };
        let make = self.make.take().unwrap();
        self.adapter = Some(make(&mut env, &host));
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.inject();
        if let Some(adapter) = &mut self.adapter {
            adapter.update_if_active(updater);
        }
    }

    pub fn set_focus(&mut self, _is_focused: bool) {}

    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {}
}

/// GameActivity's surface view, or `None` for another activity.
fn game_activity_view<'l>(env: &mut JNIEnv<'l>, activity: &JObject) -> Option<JObject<'l>> {
    let view = env
        .get_field(
            activity,
            "mSurfaceView",
            "Lcom/google/androidgamesdk/GameActivity$InputEnabledSurfaceView;",
        )
        .and_then(|v| v.l());
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    view.ok()
}
