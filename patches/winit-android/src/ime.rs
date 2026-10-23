//! leaf patch: input method events from the application's own view.
//!
//! A `NativeActivity` gives an input method no `InputConnection`, so winit
//! has nothing to report `Ime` events from, and a soft keyboard can only send
//! keys. An application that gives the input method a connection of its own
//! — a Java `View` with `onCreateInputConnection`, say — hands what it hears
//! here with [`send`], from any thread, and the event loop delivers each one
//! to the window as `WindowEvent::Ime`, in order, as the other backends
//! report their input methods.

use std::collections::VecDeque;
use std::sync::Mutex;

use android_activity::AndroidAppWaker;
pub use winit_core::event::Ime;

static QUEUE: Mutex<VecDeque<Ime>> = Mutex::new(VecDeque::new());
static WAKER: Mutex<Option<AndroidAppWaker>> = Mutex::new(None);

/// Deliver `ime` to the window as `WindowEvent::Ime`, waking the event loop
/// for it. Events sent before the event loop is made wait for it.
pub fn send(ime: Ime) {
    QUEUE.lock().unwrap().push_back(ime);
    if let Some(waker) = WAKER.lock().unwrap().as_ref() {
        waker.wake();
    }
}

pub(crate) fn set_waker(waker: AndroidAppWaker) {
    *WAKER.lock().unwrap() = Some(waker);
}

pub(crate) fn pending() -> bool {
    !QUEUE.lock().unwrap().is_empty()
}

pub(crate) fn take() -> VecDeque<Ime> {
    std::mem::take(&mut *QUEUE.lock().unwrap())
}
