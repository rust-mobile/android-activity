use std::hash::Hash;
use std::sync::RwLock;
use std::time::Duration;
use std::{os::unix::prelude::RawFd, sync::Arc};

use ndk::asset::AssetManager;
// TODO: import FdEvent and avoid depending on ndk Looper abstraction in case we want to
// support using epoll directly in the future.
use ndk::looper::FdEvent;
use ndk::native_window::NativeWindow;

#[cfg(not(target_os = "android"))]
compile_error!("android-activity only supports compiling for Android");

#[cfg(all(feature = "game-activity", feature = "native-activity"))]
compile_error!(
    "The \"game-activity\" and \"native-activity\" features cannot be enabled at the same time"
);
#[cfg(all(
    not(any(feature = "game-activity", feature = "native-activity")),
    not(doc)
))]
compile_error!("Either \"game-activity\" or \"native-activity\" must be enabled as features");

#[cfg(any(feature = "native-activity", doc))]
mod native_activity;
#[cfg(any(feature = "native-activity", doc))]
use native_activity as activity_impl;

#[cfg(feature = "game-activity")]
mod game_activity;
#[cfg(feature = "game-activity")]
use game_activity as activity_impl;

pub use activity_impl::input;

mod config;
pub use config::ConfigurationRef;

mod util;

// Note: unlike in ndk-glue this has signed components (consistent
// with Android's ARect) which generally allows for representing
// rectangles with a negative/off-screen origin. Even though this
// is currently just used to represent the content rect (that probably
// wouldn't have any negative components) we keep the generality
// since this is a primitive type that could potentially be used
// for more things in the future.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub type StateSaver<'a> = activity_impl::StateSaver<'a>;
pub type StateLoader<'a> = activity_impl::StateLoader<'a>;

#[non_exhaustive]
#[derive(Debug)]
pub enum MainEvent<'a> {
    /// New input events are available via [`AndroidApp::input_events()`]
    ///
    /// _Note: Even if more input is received this event will not be resent
    /// until [`AndroidApp::input_events()`] has been called, which enables
    /// applications to batch up input processing without there being lots of
    /// redundant event loop wake ups._
    ///
    /// [`AndroidApp::input_events()`]: AndroidApp::input_events
    ///
    /// # Portability
    ///
    /// This is currently only supported with `NativeActivity` with the
    /// "native-activity" feature. Applications using `GameActivity` should
    /// continue to check for input as part of their rendering updates.
    InputAvailable,

    /// Command from main thread: a new [`NativeWindow`] is ready for use.  Upon
    /// receiving this command, [`native_window()`] will return the new window
    #[non_exhaustive]
    InitWindow {},

    /// Command from main thread: the existing [`NativeWindow`] needs to be
    /// terminated.  Upon receiving this command, [`native_window()`] still
    /// returns the existing window; after returning from the [`AndroidApp::poll_events()`]
    /// callback then [`native_window()`] will return `None`.
    #[non_exhaustive]
    TerminateWindow {},

    // TODO: include the prev and new size in the event
    /// Command from main thread: the current [`NativeWindow`] has been resized.
    /// Please redraw with its new size.
    #[non_exhaustive]
    WindowResized {},

    /// Command from main thread: the current [`NativeWindow`] needs to be redrawn.
    /// You should redraw the window before the [`AndroidApp::poll_events()`]
    /// callback returns in order to avoid transient drawing glitches.
    #[non_exhaustive]
    RedrawNeeded {},

    /// Command from main thread: the content area of the window has changed,
    /// such as from the soft input window being shown or hidden.  You can
    /// get the new content rect by calling [`AndroidApp::content_rect()`]
    #[non_exhaustive]
    ContentRectChanged {},

    /// Command from main thread: the app's activity window has gained
    /// input focus.
    GainedFocus,

    /// Command from main thread: the app's activity window has lost
    /// input focus.
    LostFocus,

    /// Command from main thread: the current device configuration has changed.
    /// You can get a copy of the latest [Configuration] by calling
    /// [`AndroidApp::config()`]
    #[non_exhaustive]
    ConfigChanged {},

    /// Command from main thread: the system is running low on memory.
    /// Try to reduce your memory use.
    LowMemory,

    /// Command from main thread: the app's activity has been started.
    Start,

    /// Command from main thread: the app's activity has been resumed.
    #[non_exhaustive]
    Resume { loader: StateLoader<'a> },

    /// Command from main thread: the app should generate a new saved state
    /// for itself, to restore from later if needed.  If you have saved state,
    /// allocate it with malloc and place it in android_app.savedState with
    /// the size in android_app.savedStateSize.  The will be freed for you
    /// later.
    #[non_exhaustive]
    SaveState { saver: StateSaver<'a> },

    /// Command from main thread: the app's activity has been paused.
    Pause,

    /// Command from main thread: the app's activity has been stopped.
    Stop,

    /// Command from main thread: the app's activity is being destroyed,
    /// and waiting for the app thread to clean up and exit before proceeding.
    Destroy,

    /// Command from main thread: the app's insets have changed.
    #[non_exhaustive]
    InsetsChanged {},
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PollEvent<'a> {
    Wake,
    Timeout,
    Main(MainEvent<'a>),

    #[non_exhaustive]
    FdEvent {
        ident: i32,
        fd: RawFd,
        events: FdEvent,
        data: *mut std::ffi::c_void,
    },

    Error,
}

use activity_impl::AndroidAppInner;

pub use activity_impl::AndroidAppWaker;

#[derive(Debug, Clone)]
pub struct AndroidApp {
    pub(crate) inner: Arc<RwLock<AndroidAppInner>>,
}

impl PartialEq for AndroidApp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for AndroidApp {}

impl Hash for AndroidApp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl AndroidApp {
    #[cfg_attr(docsrs, doc(cfg(feature = "native-activity")))]
    #[cfg(feature = "native-activity")]
    pub(crate) fn native_activity(&self) -> *const ndk_sys::ANativeActivity {
        self.inner.read().unwrap().native_activity()
    }

    /// Queries the current [`NativeWindow`] for the application.
    ///
    /// This will only return `Some(window)` between
    /// [`AndroidAppMainEvent::InitWindow`] and [`AndroidAppMainEvent::TerminateWindow`]
    /// events.
    pub fn native_window<'a>(&self) -> Option<NativeWindow> {
        self.inner.read().unwrap().native_window()
    }

    /// Calls [`ALooper_pollAll`] on the looper associated with this AndroidApp as well
    /// as processing any events (such as lifecycle events) via the given `callback`.
    ///
    /// It's important to use this API for polling, and not call [`ALooper_pollAll`] directly since
    /// some events require pre- and post-processing either side of the callback. For correct
    /// behavior events should be handled immediately, before returning from the callback and
    /// not simply queued for batch processing later. For example the existing [`NativeWindow`]
    /// is accessible during a [`MainEvent::TerminateWindow`] callback and will be
    /// set to `None` once the callback returns, and this is also synchronized with the Java
    /// main thread. The [`MainEvent::SaveState`] event is also synchronized with the
    /// Java main thread.
    pub fn poll_events<F>(&self, timeout: Option<Duration>, callback: F)
    where
        F: FnMut(PollEvent),
    {
        self.inner.read().unwrap().poll_events(timeout, callback);
    }

    /// Creates a means to wake up the main loop while it is blocked waiting for
    /// events within [`poll_events()`].
    ///
    /// Internally this uses [`ALooper_wake`] on the looper associated with this
    /// [AndroidApp].
    pub fn create_waker(&self) -> activity_impl::AndroidAppWaker {
        self.inner.read().unwrap().create_waker()
    }

    /// Returns a (cheaply clonable) reference to this application's [`Configuration`]
    pub fn config(&self) -> ConfigurationRef {
        self.inner.read().unwrap().config()
    }

    /// Queries the current content rectangle of the window; this is the area where the
    /// window's content should be placed to be seen by the user.
    pub fn content_rect(&self) -> Rect {
        self.inner.read().unwrap().content_rect()
    }

    /// Queries the Asset Manager instance for the application.
    ///
    /// Use this to access binary assets bundled inside your application's .apk file.
    pub fn asset_manager(&self) -> AssetManager {
        self.inner.read().unwrap().asset_manager()
    }

    pub fn enable_motion_axis(&self, axis: input::Axis) {
        self.inner.write().unwrap().enable_motion_axis(axis);
    }

    pub fn disable_motion_axis(&self, axis: input::Axis) {
        self.inner.write().unwrap().disable_motion_axis(axis);
    }

    pub fn input_events<'b, F>(&self, callback: F)
    where
        F: FnMut(&input::InputEvent),
    {
        self.inner.read().unwrap().input_events(callback);
    }

    /// The user-visible SDK version of the framework
    ///
    /// Also referred to as [`Build.VERSION_CODES`](https://developer.android.com/reference/android/os/Build.VERSION_CODES)
    pub fn sdk_version() -> i32 {
        let mut prop = android_properties::getprop("ro.build.version.sdk");
        if let Some(val) = prop.value() {
            i32::from_str_radix(&val, 10).expect("Failed to parse ro.build.version.sdk property")
        } else {
            panic!("Couldn't read ro.build.version.sdk system property");
        }
    }

    /// Path to this application's internal data directory
    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().internal_data_path()
    }

    /// Path to this application's external data directory
    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().external_data_path()
    }

    /// Path to the directory containing the application's OBB files (if any).
    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().obb_path()
    }
}

#[test]
fn test_app_is_send_sync() {
    fn needs_send_sync<T: Send + Sync>() {}
    needs_send_sync::<AndroidApp>();
}
