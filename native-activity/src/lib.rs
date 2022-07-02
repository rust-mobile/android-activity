use log::{Level, error, info, trace};
use ndk::asset::AssetManager;
use ndk::configuration::Configuration;
use ndk::input_queue::InputQueue;
use ndk::looper::{FdEvent};
use ndk::native_window::NativeWindow;
use ndk_sys::ALooper_wake;
use ndk_sys::{ALooper, ALooper_pollAll};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::Deref;
use std::os::raw;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::time::Duration;
use std::{thread, ptr};
use std::os::unix::prelude::*;
use lazy_static::lazy_static;

#[cfg(not(any(target_os = "android", feature = "test")))]
compile_error!("android-ndk-sys only supports compiling for Android");

mod ffi;

pub mod input {
    pub use ndk::event::{
        InputEvent, Source, MetaState,
        MotionEvent, Pointer, MotionAction, Axis, ButtonState, EdgeFlags, MotionEventFlags,
        KeyEvent, KeyAction, Keycode, KeyEventFlags
    };
}

//pub mod input;

// We provide a side-band way to access the global AndroidApp
// via `android_app()` since there's no FFI safe way of calling
// an `extern "C" android_main()` with the AndroidApp while it's
// based on an `Arc<RwLock<>>` (without extra steps to pass an
// ffi safe handle/pointer).
//
// Technically is should actually be safe to pass the app as an
// argument, regardless of the unspecified layout for FFI, since
// we can assume that android_main is compiled at the same time
// by the same compiler as part of the same cdylib, so we could
// consider removing this static global if there's a good way to
// squash the compiler warnings.
//
// Note: for winit if we removed the `android_app()` getter then
// apps would have to explicitly pass the AndroidApp via an
// android specific event loop builder api /
// PlatformSpecificEventLoopAttributes - so having this global
// getter also helps keep simple winit usage portable.
static mut ANDROID_APP: Option<AndroidApp> = None;

pub type InputEvent = ndk::event::InputEvent;

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

// XXX: NativeWindow is a ref-counted object but the NativeWindow rust API
// doesn't currently implement Clone() in terms of acquiring a reference
// and Drop() in terms of releasing a reference.

/// A reference to a `NativeWindow`, used for rendering
pub struct NativeWindowRef {
    inner: NativeWindow
}
impl NativeWindowRef {
    pub fn new(native_window: &NativeWindow) -> Self {
        unsafe { ndk_sys::ANativeWindow_acquire(native_window.ptr().as_ptr()); }
        Self { inner: native_window.clone() }
    }
}
impl Drop for NativeWindowRef {
    fn drop(&mut self) {
        unsafe { ndk_sys::ANativeWindow_release(self.inner.ptr().as_ptr()) }
    }
}
impl Deref for NativeWindowRef {
    type Target = NativeWindow;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// The only time it's safe to update the android_app->savedState pointer is
// while handling a SaveState event, so this API is only exposed for those
// events...
#[derive(Debug)]
pub struct StateSaver<'a> {
    app: &'a AndroidApp,
}

impl<'a> StateSaver<'a> {
    pub fn store(&self, state: &'a [u8]) {

        // android_native_app_glue specifically expects savedState to have been allocated
        // via libc::malloc since it will automatically handle freeing the data once it
        // has been handed over to the Java Activity / main thread.
        unsafe {
            let app_ptr = self.app.ptr.as_ptr();

            // In case the application calls store() multiple times for some reason we
            // make sure to free any pre-existing state...
            if (*app_ptr).savedState != ptr::null_mut() {
                libc::free((*app_ptr).savedState);
                (*app_ptr).savedState = ptr::null_mut();
                (*app_ptr).savedStateSize = 0;
            }

            let buf = libc::malloc(state.len());
            if buf == ptr::null_mut() {
                panic!("Failed to allocate save_state buffer");
            }

            // Since it's a byte array there's no special alignment requirement here.
            //
            // Since we re-define `buf` we ensure it's not possible to access the buffer
            // via its original pointer for the lifetime of the slice.
            {
                let buf: &mut [u8] = std::slice::from_raw_parts_mut(buf.cast(), state.len());
                buf.copy_from_slice(state);
            }

            (*app_ptr).savedState = buf;
            (*app_ptr).savedStateSize = state.len() as u64;
        }
    }
}

#[derive(Debug)]
pub struct StateLoader<'a> {
    app: &'a AndroidApp,
}
impl<'a> StateLoader<'a> {
    pub fn load(&self) -> Option<Vec<u8>> {
        unsafe {
            let app_ptr = self.app.ptr.as_ptr();
            if (*app_ptr).savedState != ptr::null_mut() && (*app_ptr).savedStateSize > 0 {
                let buf: &mut [u8] = std::slice::from_raw_parts_mut((*app_ptr).savedState.cast(), (*app_ptr).savedStateSize as usize);
                let state = buf.to_vec();
                Some(state)
            } else {
                None
            }
        }
    }
}

// TODO: make more of these into non_exhaustive structs so it's possible to
// extend what data is passed to each event without breaking the API..
#[non_exhaustive]
#[derive(Debug)]
pub enum MainEvent<'a> {

    // XXX: No need to expose for now, and isn't applicable with GameActivity
    // Command from main thread: the input queue has changed.
    // Note: since the internal `AInputQueue` is not exposed directly, applications
    // won't typically need to react to this.
    //InputQueueChanged,

    /// Command from main thread: a new [`NativeWindow`] is ready for use.  Upon
    /// receiving this command, [`native_window()`] will return the new window
    #[non_exhaustive]
    InitWindow { },

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
    ContentRectChanged,

    /// Command from main thread: the app's activity window has gained
    /// input focus.
    GainedFocus,

    /// Command from main thread: the app's activity window has lost
    /// input focus.
    LostFocus,

    /// Command from main thread: the current device configuration has changed.
    /// You can get a copy of the latest [Configuration] by calling
    /// [`AndroidApp::config()`]
    ConfigChanged,

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
    FdEvent { ident: i32, fd: RawFd, events: FdEvent, data: *mut std::ffi::c_void },

    Error
}

#[derive(Clone)]
pub struct AndroidAppWaker {
    // The looper pointer is owned by the android_app and effectively
    // has a 'static lifetime, and the ALooper_wake C API is thread
    // safe, so this can be cloned safely and is send + sync safe
    looper: NonNull<ALooper>
}
unsafe impl Send for AndroidAppWaker {}
unsafe impl Sync for AndroidAppWaker {}

impl AndroidAppWaker {
    pub fn wake(&self) {
        unsafe { ALooper_wake(self.looper.as_ptr()); }
    }
}
#[derive(Debug, Clone)]
pub struct AndroidApp {
    inner: Arc<AndroidAppInner>
}

impl Deref for AndroidApp {
    type Target = Arc<AndroidAppInner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug)]
pub struct AndroidAppInner {
    ptr: NonNull<ffi::android_app>,
    config: RwLock<Configuration>,
    native_window: RwLock<Option<NativeWindow>>,
}

impl AndroidApp {

    pub(crate) unsafe fn from_ptr(ptr: NonNull<ffi::android_app>) -> Self {

        // Note: we don't use from_ptr since we don't own the android_app.config
        // and need to keep in mind that the Drop handler is going to call
        // AConfiguration_delete()
        //
        // Whenever we get a ConfigChanged notification we synchronize this
        // config state with a deep copy.
        let config = Configuration::clone_from_ptr(NonNull::new_unchecked((*ptr.as_ptr()).config));

        Self {
            inner: Arc::new(AndroidAppInner {
                ptr,
                config: RwLock::new(config),
                native_window: Default::default()
            })
        }
    }

    pub(crate) fn native_activity(&self) -> *const ndk_sys::ANativeActivity {
        unsafe {
            let app_ptr = self.ptr.as_ptr();
            (*app_ptr).activity.cast()
        }
    }

    /// Queries the current [`NativeWindow`] for the application.
    ///
    /// This will only return `Some(window)` between
    /// [`AndroidAppMainEvent::InitWindow`] and [`AndroidAppMainEvent::TerminateWindow`]
    /// events.
    pub fn native_window<'a>(&self) -> Option<NativeWindowRef> {
        let guard = self.native_window.read().unwrap();
        if let Some(ref window) = *guard {
            Some(NativeWindowRef::new(window))
        } else {
            None
        }
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
    ///
    /// # Safety
    /// This API must only be called from the applications main thread
    pub fn poll_events<F>(&self, timeout: Option<Duration>, mut callback: F)
        where F: FnMut(PollEvent)
    {
        trace!("poll_events");

        unsafe {
            let app_ptr = self.ptr;

            let mut fd: i32 = 0;
            let mut events: i32 = 0;
            let mut source: *mut core::ffi::c_void = ptr::null_mut();

            let timeout_milliseconds = if let Some(timeout) = timeout { timeout.as_millis() as i32 } else { -1 };
            info!("Calling ALooper_pollAll, timeout = {timeout_milliseconds}");
            let id = ALooper_pollAll(timeout_milliseconds, &mut fd, &mut events, &mut source as *mut *mut core::ffi::c_void);
            info!("pollAll id = {id}");
            match id {
                ffi::ALOOPER_POLL_WAKE => {
                    trace!("ALooper_pollAll returned POLL_WAKE");
                    callback(PollEvent::Wake);
                }
                ffi::ALOOPER_POLL_CALLBACK => {
                    // ALooper_pollAll is documented to handle all callback sources internally so it should
                    // never return a _CALLBACK source id...
                    error!("Spurious ALOOPER_POLL_CALLBACK from ALopper_pollAll() (ignored)");
                }
                ffi::ALOOPER_POLL_TIMEOUT => {
                    trace!("ALooper_pollAll returned POLL_TIMEOUT");
                    callback(PollEvent::Timeout);
                }
                ffi::ALOOPER_POLL_ERROR => {
                    trace!("ALooper_pollAll returned POLL_ERROR");
                    callback(PollEvent::Error);

                    // Considering that this API is quite likely to be used in `android_main`
                    // it's rather unergonomic to require the call to unwrap a Result for each
                    // call to poll_events(). Alternatively we could maybe even just panic!()
                    // here, while it's hard to imagine practically being able to recover
                    //return Err(LooperError);
                }
                id if id >= 0 => {
                    match id as u32 {
                        ffi::LOOPER_ID_MAIN => {
                            trace!("ALooper_pollAll returned ID_MAIN");
                            let source: *mut ffi::android_poll_source = source.cast();
                            if source != ptr::null_mut() {
                                let cmd_i = ffi::android_app_read_cmd(app_ptr.as_ptr());

                                let cmd = match cmd_i as u32 {
                                    // We don't forward info about the AInputQueue to apps since it's
                                    // an implementation details that's also not compatible with
                                    // GameActivity
                                    ffi::APP_CMD_INPUT_CHANGED => None,

                                    ffi::APP_CMD_INIT_WINDOW => Some(MainEvent::InitWindow {}),
                                    ffi::APP_CMD_TERM_WINDOW => Some(MainEvent::TerminateWindow {}),
                                    ffi::APP_CMD_WINDOW_RESIZED => Some(MainEvent::WindowResized {}),
                                    ffi::APP_CMD_WINDOW_REDRAW_NEEDED => Some(MainEvent::RedrawNeeded {}),
                                    ffi::APP_CMD_CONTENT_RECT_CHANGED => Some(MainEvent::ContentRectChanged),
                                    ffi::APP_CMD_GAINED_FOCUS => Some(MainEvent::GainedFocus),
                                    ffi::APP_CMD_LOST_FOCUS => Some(MainEvent::LostFocus),
                                    ffi::APP_CMD_CONFIG_CHANGED => Some(MainEvent::ConfigChanged),
                                    ffi::APP_CMD_LOW_MEMORY => Some(MainEvent::LowMemory),
                                    ffi::APP_CMD_START => Some(MainEvent::Start),
                                    ffi::APP_CMD_RESUME => Some(MainEvent::Resume { loader: StateLoader { app: &self } }),
                                    ffi::APP_CMD_SAVE_STATE => Some(MainEvent::SaveState { saver: StateSaver { app: &self } }),
                                    ffi::APP_CMD_PAUSE => Some(MainEvent::Pause),
                                    ffi::APP_CMD_STOP => Some(MainEvent::Stop),
                                    ffi::APP_CMD_DESTROY => Some(MainEvent::Destroy),

                                    //ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_INSETS_CHANGED => MainEvent::InsetsChanged {},
                                    _ => unreachable!()
                                };

                                trace!("Calling android_app_pre_exec_cmd({cmd_i})");
                                ffi::android_app_pre_exec_cmd(app_ptr.as_ptr(), cmd_i);

                                if let Some(cmd) = cmd {
                                    trace!("Read ID_MAIN command {cmd_i} = {cmd:?}");
                                    match cmd {
                                        MainEvent::ConfigChanged => {
                                            *self.config.write().unwrap() =
                                                Configuration::clone_from_ptr(NonNull::new_unchecked((*app_ptr.as_ptr()).config));
                                        }
                                        MainEvent::InitWindow { .. } => {
                                            let win_ptr = (*app_ptr.as_ptr()).window;
                                            *self.native_window.write().unwrap() =
                                                Some(NativeWindow::from_ptr(NonNull::new(win_ptr).unwrap()));
                                        }
                                        MainEvent::TerminateWindow { .. } => {
                                            *self.native_window.write().unwrap() = None;
                                        }
                                        _ => {}
                                    }

                                    trace!("Invoking callback for ID_MAIN command = {:?}", cmd);
                                    callback(PollEvent::Main(cmd));
                                }

                                trace!("Calling android_app_post_exec_cmd({cmd_i})");
                                ffi::android_app_post_exec_cmd(app_ptr.as_ptr(), cmd_i);
                            } else {
                                panic!("ALooper_pollAll returned ID_MAIN event with NULL android_poll_source!");
                            }
                        }
                        ffi::LOOPER_ID_INPUT => {
                            trace!("ALooper_pollAll returned ID_INPUT");
                            // For now we don't forward notifications of input events specifically, we just
                            // forward the notifications as a wake up, and assume the application main loop
                            // will unconditionally check events for each iteration of it's event loop
                            //
                            // (Specifically notifying when input events are received would be inconsistent
                            // with the current design of GameActivity input handling which we want to stay
                            // compatible with))
                            //
                            // XXX: Actually it was a bad idea to emit a Wake for input since applications
                            // are likely to _not_ consider that on its own a cause to redraw and it could
                            // end up spamming enough wake ups to interfere with other events that would
                            // trigger a redraw + input handling
                            //callback(PollEvent::Wake);
                        }
                        _ => {
                            let events = FdEvent::from_bits(events as u32)
                                .expect(&format!("Spurious ALooper_pollAll event flags {:#04x}", events as u32));
                            trace!("Custom ALooper event source: id = {id}, fd = {fd}, events = {events:?}, data = {source:?}");
                            callback(PollEvent::FdEvent{ ident: id, fd: fd as RawFd, events, data: source });
                        }
                    }
                }
                _ => {
                    error!("Spurious ALooper_pollAll return value {id} (ignored)");
                }
            }
        }
    }

    /// Creates a means to wake up the main loop while it is blocked waiting for
    /// events within [`poll_events()`].
    ///
    /// Internally this uses [`ALooper_wake`] on the looper associated with this
    /// [AndroidApp].
    ///
    /// # Safety
    /// This API can be used from any thread
    pub fn create_waker(&self) -> AndroidAppWaker {
        unsafe {
            // From the application's pov we assume the app_ptr and looper pointer
            // have static lifetimes and we can safely assume they are never NULL.
            let app_ptr = self.ptr.as_ptr();
            AndroidAppWaker { looper: NonNull::new_unchecked((*app_ptr).looper) }
        }
    }

    /// Returns a deep copy of this application's [`Configuration`]
    pub fn config(&self) -> Configuration {
        self.config.read().unwrap().clone()
    }

    /// Queries the current content rectangle of the window; this is the area where the
    /// window's content should be placed to be seen by the user.
    ///
    /// # Safety
    /// This API must only be called from the applications main thread
    pub fn content_rect(&self) -> Rect {
        unsafe {
            let app_ptr = self.ptr.as_ptr();
            Rect {
                left: (*app_ptr).contentRect.left,
                right: (*app_ptr).contentRect.right,
                top: (*app_ptr).contentRect.top,
                bottom: (*app_ptr).contentRect.bottom,
            }
        }
    }

    /// Queries the Asset Manager instance for the application.
    ///
    /// Use this to access binary assets bundled inside your application's .apk file.
    ///
    /// # Safety
    /// This API must only be called from the applications main thread
    pub fn asset_manager(&self) -> AssetManager {
        unsafe {
            let app_ptr = self.ptr.as_ptr();
            let am_ptr = NonNull::new_unchecked((*(*app_ptr).activity).assetManager);
            AssetManager::from_ptr(am_ptr)
        }
    }

    pub fn input_events<'b, F>(&self, mut callback: F)
        where F: FnMut(&InputEvent)
    {
        let queue = unsafe {
            let app_ptr = self.ptr.as_ptr();
            if (*app_ptr).inputQueue == ptr::null_mut() {
                return;
            }
            let queue = NonNull::new_unchecked((*app_ptr).inputQueue);
            InputQueue::from_ptr(queue)
        };

        info!("collect_events: START");
        while let Some(event) = queue.get_event() {
            info!("Got input event {event:?}");
            if let Some(event) = queue.pre_dispatch(event) {
                trace!("Pre dispatched input event {event:?}");

                callback(&event);

                // Always report events as 'handled'. This means we won't get
                // so called 'fallback' events generated (such as converting trackball
                // events into emulated keypad events), but we could conceivably
                // implement similar emulation somewhere else in the stack if
                // necessary, and this will be more consistent with the GameActivity
                // input handling that doesn't do any kind of emulation.
                info!("Finishing input event {event:?}");
                queue.finish_event(event, true);
            }
        }
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

    fn try_get_path_from_ptr(path: *const u8) -> Option<std::path::PathBuf> {
        if path == ptr::null() { return None; }
        let cstr = unsafe {
            let cstr_slice = CStr::from_ptr(path);
            cstr_slice.to_str().ok()?
        };
        if cstr.len() == 0 { return None; }
        Some(std::path::PathBuf::from(cstr))
    }

    /// Path to this application's internal data directory
    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { Self::try_get_path_from_ptr((*na).internalDataPath.cast()) }
    }

    /// Path to this application's external data directory
    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { Self::try_get_path_from_ptr((*na).externalDataPath.cast()) }
    }

    /// Path to the directory containing the application's OBB files (if any).
    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { Self::try_get_path_from_ptr((*na).obbPath.cast()) }
    }
}

/// Gets the global [`AndroidApp`] for this process
pub fn android_app() -> AndroidApp {
    if let Some(app) = unsafe { &ANDROID_APP } {
        return app.clone()
    } else {
        unreachable!()
    }
}

// Rust doesn't give us a clean way to directly export symbols from C/C++
// so we rename the C/C++ symbols and re-export this entrypoint from
// Rust...
//
// https://github.com/rust-lang/rfcs/issues/2771
extern "C" {
    pub fn ANativeActivity_onCreate_C(
        activity: *mut std::os::raw::c_void,
        savedState: *mut ::std::os::raw::c_void,
        savedStateSize: usize,
    );

    pub fn android_main();
}

#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut std::os::raw::c_void,
    saved_state: *mut std::os::raw::c_void,
    saved_state_size: usize,
) {
    ANativeActivity_onCreate_C(activity, saved_state, saved_state_size);
}

fn android_log(level: Level, tag: &CStr, msg: &CStr) {
    let prio = match level {
        Level::Error => ndk_sys::android_LogPriority_ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority_ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority_ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority_ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority_ANDROID_LOG_VERBOSE,
    };
    unsafe {
        ndk_sys::__android_log_write(prio as raw::c_int, tag.as_ptr(), msg.as_ptr());
    }
}

// This is a spring board between android_native_app_glue and the user's
// `app_main` function. This is run on a dedicated thread spawned
// by android_native_app_glue.
#[no_mangle]
pub unsafe extern "C" fn _rust_glue_entry(app: *mut ffi::android_app) {

    // Maybe make this stdout/stderr redirection an optional / opt-in feature?...
    let mut logpipe: [RawFd; 2] = Default::default();
    libc::pipe(logpipe.as_mut_ptr());
    libc::dup2(logpipe[1], libc::STDOUT_FILENO);
    libc::dup2(logpipe[1], libc::STDERR_FILENO);
    thread::spawn(move || {
        let tag = CStr::from_bytes_with_nul(b"RustStdoutStderr\0").unwrap();
        let file = File::from_raw_fd(logpipe[0]);
        let mut reader = BufReader::new(file);
        let mut buffer = String::new();
        loop {
            buffer.clear();
            if let Ok(len) = reader.read_line(&mut buffer) {
                if len == 0 {
                    break;
                } else if let Ok(msg) = CString::new(buffer.clone()) {
                    android_log(Level::Info, tag, &msg);
                }
            }
        }
    });

    let app = AndroidApp::from_ptr(NonNull::new(app).unwrap());

    let na = app.native_activity();
    let jvm = (*na).vm;
    let activity = (*na).clazz; // Completely bogus name; this is the _instance_ not class pointer
    ndk_context::initialize_android_context(jvm.cast(), activity.cast());

    ANDROID_APP = Some(app.clone());

    // Since this is a newly spawned thread then the JVM hasn't been attached
    // to the thread yet. Attach before calling the applications main function
    // so they can safely make JNI calls
    let mut jenv_out: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Some(attach_current_thread) = (*(*jvm)).AttachCurrentThread {
        attach_current_thread(jvm, &mut jenv_out, std::ptr::null_mut());
    }

    // XXX: If we were in control of the Java Activity subclass then
    // we could potentially run the android_main function via a Java native method
    // springboard (e.g. call an Activity subclass method that calls a jni native
    // method that then just calls android_main()) that would make sure there was
    // a Java frame at the base of our call stack which would then be recognised
    // when calling FindClass to lookup a suitable classLoader, instead of
    // defaulting to the system loader. Without this then it's difficult for native
    // code to look up non-standard Java classes.
    android_main();

    // Since this is a newly spawned thread then the JVM hasn't been attached
    // to the thread yet. Attach before calling the applications main function
    // so they can safely make JNI calls
    if let Some(detach_current_thread) = (*(*jvm)).DetachCurrentThread {
        detach_current_thread(jvm);
    }

    ANDROID_APP = None;

    ndk_context::release_android_context();
}