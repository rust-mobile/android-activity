#![cfg(any(feature = "native-activity", doc))]

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::Deref;
use std::os::raw;
use std::os::unix::prelude::*;
use std::ptr::NonNull;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{ptr, thread};

use log::{error, info, trace, Level};

use ndk_sys::ALooper_wake;
use ndk_sys::{ALooper, ALooper_pollAll};

use ndk::asset::AssetManager;
use ndk::configuration::Configuration;
use ndk::input_queue::InputQueue;
use ndk::native_window::NativeWindow;

use crate::{
    util, AndroidApp, ConfigurationRef, InputStatus, MainEvent, PollEvent, Rect, WindowManagerFlags,
};

mod ffi;

pub mod input {
    pub use ndk::event::{
        Axis, ButtonState, EdgeFlags, KeyAction, KeyEvent, KeyEventFlags, Keycode, MetaState,
        MotionAction, MotionEvent, MotionEventFlags, Pointer, Source,
    };

    // We use our own wrapper type for input events to have better consistency
    // with GameActivity and ensure the enum can be extended without needing a
    // semver bump
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum InputEvent {
        MotionEvent(self::MotionEvent),
        KeyEvent(self::KeyEvent),
    }
}

// The only time it's safe to update the android_app->savedState pointer is
// while handling a SaveState event, so this API is only exposed for those
// events...
#[derive(Debug)]
pub struct StateSaver<'a> {
    app: &'a AndroidAppInner,
}

impl<'a> StateSaver<'a> {
    pub fn store(&self, state: &'a [u8]) {
        // android_native_app_glue specifically expects savedState to have been allocated
        // via libc::malloc since it will automatically handle freeing the data once it
        // has been handed over to the Java Activity / main thread.
        unsafe {
            let app_ptr = self.app.native_app.as_ptr();

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
            (*app_ptr).savedStateSize = state.len() as _;
        }
    }
}

#[derive(Debug)]
pub struct StateLoader<'a> {
    app: &'a AndroidAppInner,
}
impl<'a> StateLoader<'a> {
    pub fn load(&self) -> Option<Vec<u8>> {
        unsafe {
            let app_ptr = self.app.native_app.as_ptr();
            if (*app_ptr).savedState != ptr::null_mut() && (*app_ptr).savedStateSize > 0 {
                let buf: &mut [u8] = std::slice::from_raw_parts_mut(
                    (*app_ptr).savedState.cast(),
                    (*app_ptr).savedStateSize as usize,
                );
                let state = buf.to_vec();
                Some(state)
            } else {
                None
            }
        }
    }
}

#[derive(Clone)]
pub struct AndroidAppWaker {
    // The looper pointer is owned by the android_app and effectively
    // has a 'static lifetime, and the ALooper_wake C API is thread
    // safe, so this can be cloned safely and is send + sync safe
    looper: NonNull<ALooper>,
}
unsafe impl Send for AndroidAppWaker {}
unsafe impl Sync for AndroidAppWaker {}

impl AndroidAppWaker {
    pub fn wake(&self) {
        unsafe {
            ALooper_wake(self.looper.as_ptr());
        }
    }
}

impl AndroidApp {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<ffi::android_app>) -> AndroidApp {
        // Note: we don't use from_ptr since we don't own the android_app.config
        // and need to keep in mind that the Drop handler is going to call
        // AConfiguration_delete()
        let config = Configuration::clone_from_ptr(NonNull::new_unchecked((*ptr.as_ptr()).config));

        AndroidApp {
            inner: Arc::new(RwLock::new(AndroidAppInner {
                native_app: NativeAppGlue { ptr },
                config: ConfigurationRef::new(config),
                native_window: Default::default(),
            })),
        }
    }
}

#[derive(Debug)]
struct NativeAppGlue {
    ptr: NonNull<ffi::android_app>,
}
impl Deref for NativeAppGlue {
    type Target = NonNull<ffi::android_app>;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}
unsafe impl Send for NativeAppGlue {}
unsafe impl Sync for NativeAppGlue {}

#[derive(Debug)]
pub(crate) struct AndroidAppInner {
    native_app: NativeAppGlue,
    config: ConfigurationRef,
    native_window: RwLock<Option<NativeWindow>>,
}

impl AndroidAppInner {
    pub(crate) fn native_activity(&self) -> *const ndk_sys::ANativeActivity {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            (*app_ptr).activity.cast()
        }
    }

    pub fn native_window<'a>(&self) -> Option<NativeWindow> {
        self.native_window.read().unwrap().clone()
    }

    pub fn poll_events<F>(&self, timeout: Option<Duration>, mut callback: F)
    where
        F: FnMut(PollEvent),
    {
        trace!("poll_events");

        unsafe {
            let native_app = &self.native_app;

            let mut fd: i32 = 0;
            let mut events: i32 = 0;
            let mut source: *mut core::ffi::c_void = ptr::null_mut();

            let timeout_milliseconds = if let Some(timeout) = timeout {
                timeout.as_millis() as i32
            } else {
                -1
            };
            info!("Calling ALooper_pollAll, timeout = {timeout_milliseconds}");
            let id = ALooper_pollAll(
                timeout_milliseconds,
                &mut fd,
                &mut events,
                &mut source as *mut *mut core::ffi::c_void,
            );
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
                    // If we have an IO error with our pipe to the main Java thread that's surely
                    // not something we can recover from
                    panic!("ALooper_pollAll returned POLL_ERROR");
                }
                id if id >= 0 => {
                    match id as u32 {
                        ffi::LOOPER_ID_MAIN => {
                            trace!("ALooper_pollAll returned ID_MAIN");
                            let source: *mut ffi::android_poll_source = source.cast();
                            if source != ptr::null_mut() {
                                let cmd_i = ffi::android_app_read_cmd(native_app.as_ptr());

                                let cmd = match cmd_i as u32 {
                                    // We don't forward info about the AInputQueue to apps since it's
                                    // an implementation details that's also not compatible with
                                    // GameActivity
                                    ffi::APP_CMD_INPUT_CHANGED => None,

                                    ffi::APP_CMD_INIT_WINDOW => Some(MainEvent::InitWindow {}),
                                    ffi::APP_CMD_TERM_WINDOW => Some(MainEvent::TerminateWindow {}),
                                    ffi::APP_CMD_WINDOW_RESIZED => {
                                        Some(MainEvent::WindowResized {})
                                    }
                                    ffi::APP_CMD_WINDOW_REDRAW_NEEDED => {
                                        Some(MainEvent::RedrawNeeded {})
                                    }
                                    ffi::APP_CMD_CONTENT_RECT_CHANGED => {
                                        Some(MainEvent::ContentRectChanged {})
                                    }
                                    ffi::APP_CMD_GAINED_FOCUS => Some(MainEvent::GainedFocus),
                                    ffi::APP_CMD_LOST_FOCUS => Some(MainEvent::LostFocus),
                                    ffi::APP_CMD_CONFIG_CHANGED => {
                                        Some(MainEvent::ConfigChanged {})
                                    }
                                    ffi::APP_CMD_LOW_MEMORY => Some(MainEvent::LowMemory),
                                    ffi::APP_CMD_START => Some(MainEvent::Start),
                                    ffi::APP_CMD_RESUME => Some(MainEvent::Resume {
                                        loader: StateLoader { app: &self },
                                    }),
                                    ffi::APP_CMD_SAVE_STATE => Some(MainEvent::SaveState {
                                        saver: StateSaver { app: &self },
                                    }),
                                    ffi::APP_CMD_PAUSE => Some(MainEvent::Pause),
                                    ffi::APP_CMD_STOP => Some(MainEvent::Stop),
                                    ffi::APP_CMD_DESTROY => Some(MainEvent::Destroy),

                                    //ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_INSETS_CHANGED => MainEvent::InsetsChanged {},
                                    _ => unreachable!(),
                                };

                                trace!("Calling android_app_pre_exec_cmd({cmd_i})");
                                ffi::android_app_pre_exec_cmd(native_app.as_ptr(), cmd_i);

                                if let Some(cmd) = cmd {
                                    trace!("Read ID_MAIN command {cmd_i} = {cmd:?}");
                                    match cmd {
                                        MainEvent::ConfigChanged { .. } => {
                                            self.config.replace(Configuration::clone_from_ptr(
                                                NonNull::new_unchecked(
                                                    (*native_app.as_ptr()).config,
                                                ),
                                            ));
                                        }
                                        MainEvent::InitWindow { .. } => {
                                            let win_ptr = (*native_app.as_ptr()).window;
                                            // It's important that we use ::clone_from_ptr() here
                                            // because NativeWindow has a Drop implementation that
                                            // will unconditionally _release() the native window
                                            *self.native_window.write().unwrap() =
                                                Some(NativeWindow::clone_from_ptr(
                                                    NonNull::new(win_ptr).unwrap(),
                                                ));
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
                                ffi::android_app_post_exec_cmd(native_app.as_ptr(), cmd_i);
                            } else {
                                panic!("ALooper_pollAll returned ID_MAIN event with NULL android_poll_source!");
                            }
                        }
                        ffi::LOOPER_ID_INPUT => {
                            trace!("ALooper_pollAll returned ID_INPUT");

                            // To avoid spamming the application with event loop iterations notifying them of
                            // input events then we only send one `InputAvailable` per iteration of input
                            // handling. We re-attach the looper when the application calls
                            // `AndroidApp::input_events()`
                            ffi::android_app_detach_input_queue_looper(native_app.as_ptr());
                            callback(PollEvent::Main(MainEvent::InputAvailable))
                        }
                        _ => {
                            error!("Ignoring spurious ALooper event source: id = {id}, fd = {fd}, events = {events:?}, data = {source:?}");
                        }
                    }
                }
                _ => {
                    error!("Spurious ALooper_pollAll return value {id} (ignored)");
                }
            }
        }
    }

    pub fn create_waker(&self) -> AndroidAppWaker {
        unsafe {
            // From the application's pov we assume the app_ptr and looper pointer
            // have static lifetimes and we can safely assume they are never NULL.
            let app_ptr = self.native_app.as_ptr();
            AndroidAppWaker {
                looper: NonNull::new_unchecked((*app_ptr).looper),
            }
        }
    }

    pub fn config(&self) -> ConfigurationRef {
        self.config.clone()
    }

    pub fn content_rect(&self) -> Rect {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            Rect {
                left: (*app_ptr).contentRect.left,
                right: (*app_ptr).contentRect.right,
                top: (*app_ptr).contentRect.top,
                bottom: (*app_ptr).contentRect.bottom,
            }
        }
    }

    pub fn asset_manager(&self) -> AssetManager {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            let am_ptr = NonNull::new_unchecked((*(*app_ptr).activity).assetManager);
            AssetManager::from_ptr(am_ptr)
        }
    }

    pub fn set_window_flags(
        &self,
        add_flags: WindowManagerFlags,
        remove_flags: WindowManagerFlags,
    ) {
        let na = self.native_activity();
        let na_mut = na as *mut ndk_sys::ANativeActivity;
        unsafe {
            ffi::ANativeActivity_setWindowFlags(
                na_mut.cast(),
                add_flags.bits(),
                remove_flags.bits(),
            );
        }
    }

    // TODO: move into a trait
    pub fn show_soft_input(&self, show_implicit: bool) {
        let na = self.native_activity();
        unsafe {
            let flags = if show_implicit {
                ffi::ANATIVEACTIVITY_SHOW_SOFT_INPUT_IMPLICIT
            } else {
                0
            };
            ffi::ANativeActivity_showSoftInput(na as *mut _, flags);
        }
    }

    // TODO: move into a trait
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        let na = self.native_activity();
        unsafe {
            let flags = if hide_implicit_only {
                ffi::ANATIVEACTIVITY_HIDE_SOFT_INPUT_IMPLICIT_ONLY
            } else {
                0
            };
            ffi::ANativeActivity_hideSoftInput(na as *mut _, flags);
        }
    }

    pub fn enable_motion_axis(&self, _axis: input::Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn disable_motion_axis(&self, _axis: input::Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn input_events<'b, F>(&self, mut callback: F)
    where
        F: FnMut(&input::InputEvent) -> InputStatus,
    {
        let queue = unsafe {
            let app_ptr = self.native_app.as_ptr();
            if (*app_ptr).inputQueue == ptr::null_mut() {
                return;
            }

            // Reattach the input queue to the looper so future input will again deliver an
            // `InputAvailable` event.
            ffi::android_app_attach_input_queue_looper(app_ptr);

            let queue = NonNull::new_unchecked((*app_ptr).inputQueue);
            InputQueue::from_ptr(queue)
        };

        // Note: we basically ignore errors from get_event() currently. Looking
        // at the source code for Android's InputQueue, the only error that
        // can be returned here is 'WOULD_BLOCK', which we want to just treat as
        // meaning the queue is empty.
        //
        // ref: https://github.com/aosp-mirror/platform_frameworks_base/blob/master/core/jni/android_view_InputQueue.cpp
        //
        while let Ok(Some(event)) = queue.get_event() {
            if let Some(ndk_event) = queue.pre_dispatch(event) {
                let event = match ndk_event {
                    ndk::event::InputEvent::MotionEvent(e) => input::InputEvent::MotionEvent(e),
                    ndk::event::InputEvent::KeyEvent(e) => input::InputEvent::KeyEvent(e),
                };
                let handled = callback(&event);

                let ndk_event = match event {
                    input::InputEvent::MotionEvent(e) => ndk::event::InputEvent::MotionEvent(e),
                    input::InputEvent::KeyEvent(e) => ndk::event::InputEvent::KeyEvent(e),
                };
                queue.finish_event(
                    ndk_event,
                    match handled {
                        InputStatus::Handled => true,
                        _ => false,
                    },
                );
            }
        }
    }

    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { util::try_get_path_from_ptr((*na).internalDataPath) }
    }

    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { util::try_get_path_from_ptr((*na).externalDataPath) }
    }

    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        let na = self.native_activity();
        unsafe { util::try_get_path_from_ptr((*na).obbPath) }
    }
}

extern "C" fn android_app_main(arg: *mut libc::c_void) -> *mut libc::c_void {
    unsafe { ffi::android_app_entry(arg) as *mut _ }
}

unsafe fn android_app_create(activity: *mut ffi::ANativeActivity,
    saved_state_in: *const libc::c_void, saved_state_size: libc::size_t) -> *mut ffi::android_app
{
    let mut msgpipe: [libc::c_int; 2] = [ -1, -1 ];
    if libc::pipe(msgpipe.as_mut_ptr()) != 0 {
        panic!("could not create  Rust <-> Java IPC pipe: {}", std::io::Error::last_os_error());
    }

    // For now we need to use malloc to track saved state, while the android_native_app_glue
    // code will use free() to free the memory.
    let mut saved_state = ptr::null_mut();
    if saved_state_in != ptr::null() && saved_state_size > 0 {
        saved_state = libc::malloc(saved_state_size);
        assert!(saved_state != ptr::null_mut(), "Failed to allocate {} bytes for restoring saved application state", saved_state_size);
        libc::memcpy(saved_state, saved_state_in, saved_state_size);
    }

    let android_app = Box::into_raw(Box::new(ffi::android_app {
        userData: ptr::null_mut(),
        onAppCmd: None,
        onInputEvent: None,
        activity,
        config: ptr::null_mut(),
        savedState: saved_state,
        savedStateSize: saved_state_size,
        looper: ptr::null_mut(),
        inputQueue: ptr::null_mut(),
        window: ptr::null_mut(),
        contentRect: Rect::empty().into(),
        activityState: 0,
        destroyRequested: 0,
        mutex: libc::PTHREAD_MUTEX_INITIALIZER,
        cond: libc::PTHREAD_COND_INITIALIZER,
        msgread: msgpipe[0],
        msgwrite: msgpipe[1],
        thread: 0,
        cmdPollSource: ffi::android_poll_source { id: 0, app: ptr::null_mut(), process: None },
        inputPollSource: ffi::android_poll_source { id: 0, app: ptr::null_mut(), process: None },
        running: 0,
        stateSaved: 0,
        destroyed: 0,
        redrawNeeded: 0,
        pendingInputQueue: ptr::null_mut(),
        pendingWindow: ptr::null_mut(),
        pendingContentRect: Rect::empty().into(),
    }));

    // TODO: use std::os::spawn and drop the handle to detach instead of directly
    // using pthread_create
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::zeroed();
    libc::pthread_attr_init(attr.as_mut_ptr());
    libc::pthread_attr_setdetachstate(attr.as_mut_ptr(), libc::PTHREAD_CREATE_DETACHED);
    let mut thread = std::mem::MaybeUninit::<libc::pthread_t>::zeroed();
    libc::pthread_create(thread.as_mut_ptr(), attr.as_mut_ptr(), android_app_main, android_app as *mut _);
    let thread = thread.assume_init();
    (*android_app).thread = thread;

    // TODO: switch to std::sync::Condvar
    // Wait for thread to start.
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    while (*android_app).running == 0 {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    android_app
}

unsafe fn android_app_drop(android_app: *mut ffi::android_app) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    android_app_write_cmd(android_app, ffi::APP_CMD_DESTROY as i8);
    while !(*android_app).destroyed == 0 {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    libc::close((*android_app).msgread);
    libc::close((*android_app).msgwrite);
    libc::pthread_cond_destroy(&mut (*android_app).cond as *mut _);
    libc::pthread_mutex_destroy(&mut (*android_app).mutex as *mut _);

    let _android_app = Box::from_raw(android_app);
    // Box dropped here
}

unsafe fn android_app_write_cmd(android_app: *mut ffi::android_app, cmd: i8) {
    loop {
        match libc::write((*android_app).msgwrite, &cmd as *const _ as *const _, 1) {
            1 => break,
            -1 => {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    panic!("Failure writing android_app cmd: {}", err);
                }
            }
            count => panic!("Spurious write of {count} bytes while writing android_app cmd")
        }
    }
}

unsafe fn android_app_set_input(android_app: *mut ffi::android_app, input_queue: *mut ndk_sys::AInputQueue) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    (*android_app).pendingInputQueue = input_queue;
    android_app_write_cmd(android_app, ffi::APP_CMD_INPUT_CHANGED as i8);
    while (*android_app).inputQueue != (*android_app).pendingInputQueue {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe fn android_app_set_window(android_app: *mut ffi::android_app, window: *mut ndk_sys::ANativeWindow) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    if (*android_app).pendingWindow != ptr::null_mut() {
        android_app_write_cmd(android_app, ffi::APP_CMD_TERM_WINDOW as i8);
    }
    (*android_app).pendingWindow = window;
    if window != ptr::null_mut() {
        android_app_write_cmd(android_app, ffi::APP_CMD_INIT_WINDOW as i8);
    }
    while (*android_app).window != (*android_app).pendingWindow {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe fn android_app_set_activity_state(android_app: *mut ffi::android_app, cmd: i8) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    android_app_write_cmd(android_app, cmd);
    while (*android_app).activityState as i8 != cmd {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe extern "C" fn on_destroy(activity: *mut ffi::ANativeActivity) {
    log::debug!("Destroy: {:p}\n", activity);

    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    (*activity).instance = ptr::null_mut();
    android_app_drop(android_app);
}

unsafe extern "C" fn on_start(activity: *mut ffi::ANativeActivity) {
    log::debug!("Start: {:p}\n", activity);

    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::APP_CMD_START as i8);
}

unsafe extern "C" fn on_resume(activity: *mut ffi::ANativeActivity) {
    log::debug!("Resume: {:p}\n", activity);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::APP_CMD_RESUME as i8);
}

unsafe extern "C" fn on_save_instance_state(activity: *mut ffi::ANativeActivity, out_len: *mut libc::size_t) -> *mut libc::c_void {
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    let mut saved_state: *mut libc::c_void = ptr::null_mut();

    log::debug!("SaveInstanceState: {:p}\n", activity);
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    (*android_app).stateSaved = 0;
    android_app_write_cmd(android_app, ffi::APP_CMD_SAVE_STATE as i8);
    while (*android_app).stateSaved == 0 {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }

    if (*android_app).savedState != ptr::null_mut() {
        saved_state = (*android_app).savedState;
        *out_len = (*android_app).savedStateSize;
        (*android_app).savedState = ptr::null_mut();
        (*android_app).savedStateSize = 0;
    }

    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    return saved_state;
}

unsafe extern "C" fn on_pause(activity: *mut ffi::ANativeActivity) {
    log::debug!("Pause: {:p}\n", activity);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::APP_CMD_PAUSE as i8);
}

unsafe extern "C" fn on_stop(activity: *mut ffi::ANativeActivity) {
    log::debug!("Stop: {:p}\n", activity);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::APP_CMD_STOP as i8);
}

unsafe extern "C" fn on_configuration_changed(activity: *mut ffi::ANativeActivity) {
    log::debug!("ConfigurationChanged: {:p}\n", activity);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_write_cmd(android_app, ffi::APP_CMD_CONFIG_CHANGED as i8);
}

unsafe extern "C" fn on_low_memory(activity: *mut ffi::ANativeActivity) {
    log::debug!("LowMemory: {:p}\n", activity);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_write_cmd(android_app, ffi::APP_CMD_LOW_MEMORY as i8);
}

unsafe extern "C" fn on_window_focus_changed(activity: *mut ffi::ANativeActivity, focused: libc::c_int) {
    log::debug!("WindowFocusChanged: {:p} -- {}\n", activity, focused);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_write_cmd(android_app,
            if focused != 0 { ffi::APP_CMD_GAINED_FOCUS as i8 } else { ffi::APP_CMD_LOST_FOCUS as i8});
}

unsafe extern "C" fn on_native_window_created(activity: *mut ffi::ANativeActivity, window: *mut ndk_sys::ANativeWindow) {
    log::debug!("NativeWindowCreated: {:p} -- {:p}\n", activity, window);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_window(android_app, window);
}

unsafe extern "C" fn on_native_window_destroyed(activity: *mut ffi::ANativeActivity, window: *mut ndk_sys::ANativeWindow) {
    log::debug!("NativeWindowDestroyed: {:p} -- {:p}\n", activity, window);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_window(android_app, ptr::null_mut());
}

unsafe extern "C" fn on_input_queue_created(activity: *mut ffi::ANativeActivity, queue: *mut ndk_sys::AInputQueue) {
    log::debug!("InputQueueCreated: {:p} -- {:p}\n", activity, queue);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_input(android_app, queue);
}

unsafe extern "C" fn on_input_queue_destroyed(activity: *mut ffi::ANativeActivity, queue: *mut ndk_sys::AInputQueue) {
    log::debug!("InputQueueDestroyed: {:p} -- {:p}\n", activity, queue);
    let android_app: *mut ffi::android_app = (*activity).instance.cast();
    android_app_set_input(android_app, ptr::null_mut());
}

#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut ffi::ANativeActivity,
    saved_state: *const libc::c_void,
    saved_state_size: libc::size_t,
) {
    log::debug!("Creating: {:p}", activity);

    (*(*activity).callbacks).onDestroy = Some(on_destroy);
    (*(*activity).callbacks).onStart = Some(on_start);
    (*(*activity).callbacks).onResume = Some(on_resume);
    (*(*activity).callbacks).onSaveInstanceState = Some(on_save_instance_state);
    (*(*activity).callbacks).onPause = Some(on_pause);
    (*(*activity).callbacks).onStop = Some(on_stop);
    (*(*activity).callbacks).onConfigurationChanged = Some(on_configuration_changed);
    (*(*activity).callbacks).onLowMemory = Some(on_low_memory);
    (*(*activity).callbacks).onWindowFocusChanged = Some(on_window_focus_changed);
    (*(*activity).callbacks).onNativeWindowCreated = Some(on_native_window_created);
    (*(*activity).callbacks).onNativeWindowDestroyed = Some(on_native_window_destroyed);
    (*(*activity).callbacks).onInputQueueCreated = Some(on_input_queue_created);
    (*(*activity).callbacks).onInputQueueDestroyed = Some(on_input_queue_destroyed);

    (*activity).instance = android_app_create(activity, saved_state, saved_state_size) as *mut _;
}

fn android_log(level: Level, tag: &CStr, msg: &CStr) {
    let prio = match level {
        Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
    };
    unsafe {
        ndk_sys::__android_log_write(prio.0 as raw::c_int, tag.as_ptr(), msg.as_ptr());
    }
}

extern "Rust" {
    pub fn android_main(app: AndroidApp);
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
    android_main(app);

    // Since this is a newly spawned thread then the JVM hasn't been attached
    // to the thread yet. Attach before calling the applications main function
    // so they can safely make JNI calls
    if let Some(detach_current_thread) = (*(*jvm)).DetachCurrentThread {
        detach_current_thread(jvm);
    }

    ndk_context::release_android_context();
}
