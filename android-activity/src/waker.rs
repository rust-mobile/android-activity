use std::ptr::NonNull;
use std::task::{RawWaker, RawWakerVTable, Waker};

#[cfg(doc)]
use crate::AndroidApp;

/// A means to wake up the main thread while it is blocked waiting for I/O
pub struct AndroidAppWaker {
    looper: NonNull<ndk_sys::ALooper>,
}

impl Clone for AndroidAppWaker {
    fn clone(&self) -> Self {
        unsafe { ndk_sys::ALooper_acquire(self.looper.as_ptr()) }
        Self {
            looper: self.looper,
        }
    }
}

impl Drop for AndroidAppWaker {
    fn drop(&mut self) {
        unsafe { ndk_sys::ALooper_release(self.looper.as_ptr()) }
    }
}

unsafe impl Send for AndroidAppWaker {}
unsafe impl Sync for AndroidAppWaker {}

impl AndroidAppWaker {
    /// Acquire a ref to a looper as a means to be able to wake up the event loop
    ///
    /// # Safety
    ///
    /// The `ALooper` pointer must be valid and not null.
    pub(crate) unsafe fn new(looper: *mut ndk_sys::ALooper) -> Self {
        assert!(!looper.is_null(), "looper pointer must not be null");
        unsafe {
            // Give the waker its own reference to the looper
            ndk_sys::ALooper_acquire(looper);
            AndroidAppWaker {
                looper: NonNull::new_unchecked(looper),
            }
        }
    }

    /// Interrupts the main thread if it is blocked within [`AndroidApp::poll_events()`]
    ///
    /// If [`AndroidApp::poll_events()`] is interrupted it will invoke the poll
    /// callback with a [PollEvent::Wake][wake_event] event.
    ///
    /// [wake_event]: crate::PollEvent::Wake
    pub fn wake(&self) {
        unsafe {
            ndk_sys::ALooper_wake(self.looper.as_ptr());
        }
    }

    /// Creates a [`Waker`] that wakes up the [`AndroidApp`].
    ///
    /// This is useful for using this crate in `async` environments.
    ///
    /// [`Waker`]: std::task::Waker
    pub fn into_waker(self) -> Waker {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, drop);

        unsafe fn clone(data: *const ()) -> RawWaker {
            ndk_sys::ALooper_acquire(data as *const _ as *mut _);
            RawWaker::new(data, &VTABLE)
        }

        unsafe fn wake(data: *const ()) {
            ndk_sys::ALooper_wake(data as *const _ as *mut _)
        }

        unsafe fn drop(data: *const ()) {
            ndk_sys::ALooper_release(data as *const _ as *mut _);
        }

        // Take the existing reference to the looper and use it for the Waker
        let looper_ptr = self.looper.as_ptr() as *const ();
        std::mem::forget(self);
        unsafe { Waker::from_raw(RawWaker::new(looper_ptr, &VTABLE)) }
    }
}

impl From<AndroidAppWaker> for Waker {
    fn from(waker: AndroidAppWaker) -> Self {
        waker.into_waker()
    }
}
