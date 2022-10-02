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
            if (*app_ptr).saved_state != ptr::null_mut() {
                libc::free((*app_ptr).saved_state);
                (*app_ptr).saved_state = ptr::null_mut();
                (*app_ptr).saved_state_size = 0;
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

            (*app_ptr).saved_state = buf;
            (*app_ptr).saved_state_size = state.len() as _;
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
            if (*app_ptr).saved_state != ptr::null_mut() && (*app_ptr).saved_state_size > 0 {
                let buf: &mut [u8] = std::slice::from_raw_parts_mut(
                    (*app_ptr).saved_state.cast(),
                    (*app_ptr).saved_state_size as usize,
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

/// These are the original C structs / constants from android_native_app_glue.c naively
/// ported to Rust.
///
/// TODO: start integrating all this state directly into `AndroidApp`/`NativeAppGlue`
mod ffi {
    pub const LOOPER_ID_MAIN: libc::c_uint = 1;
    pub const LOOPER_ID_INPUT: libc::c_uint = 2;
    //pub const LOOPER_ID_USER: ::std::os::raw::c_uint = 3;

    #[derive(Clone, Copy, Eq, PartialEq, Debug)]
    pub enum AppCmd {
        InputChanged = 0,
        InitWindow = 1,
        TermWindow = 2,
        WindowResized = 3,
        WindowRedrawNeeded = 4,
        ContentRectChanged = 5,
        GainedFocus = 6,
        LostFocus = 7,
        ConfigChanged = 8,
        LowMemory = 9,
        Start = 10,
        Resume = 11,
        SaveState = 12,
        Pause = 13,
        Stop = 14,
        Destroy = 15,
    }
    impl TryFrom<i8> for AppCmd {
        type Error = ();

        fn try_from(value: i8) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(AppCmd::InputChanged),
                1 => Ok(AppCmd::InitWindow),
                2 => Ok(AppCmd::TermWindow),
                3 => Ok(AppCmd::WindowResized),
                4 => Ok(AppCmd::WindowRedrawNeeded),
                5 => Ok(AppCmd::ContentRectChanged),
                6 => Ok(AppCmd::GainedFocus),
                7 => Ok(AppCmd::LostFocus),
                8 => Ok(AppCmd::ConfigChanged),
                9 => Ok(AppCmd::LowMemory),
                10 => Ok(AppCmd::Start),
                11 => Ok(AppCmd::Resume),
                12 => Ok(AppCmd::SaveState),
                13 => Ok(AppCmd::Pause),
                14 => Ok(AppCmd::Stop),
                15 => Ok(AppCmd::Destroy),
                _ => Err(())
            }
        }
    }

    pub struct NativeActivityPollSource {
        pub id: i32,
        pub app: *mut NativeActivityGlue,
        pub process: ::std::option::Option<
            unsafe extern "C" fn(app: *mut NativeActivityGlue, source: *mut NativeActivityPollSource),
        >,
    }

    pub struct NativeActivityGlue {
        pub activity: *mut ndk_sys::ANativeActivity,
        pub config: *mut ndk_sys::AConfiguration,
        pub saved_state: *mut libc::c_void,
        pub saved_state_size: libc::size_t,
        pub looper: *mut ndk_sys::ALooper,
        pub input_queue: *mut ndk_sys::AInputQueue,
        pub window: *mut ndk_sys::ANativeWindow,
        pub content_rect: ndk_sys::ARect,
        pub activity_state: libc::c_int,
        pub destroy_requested: bool,
        pub mutex: libc::pthread_mutex_t,
        pub cond: libc::pthread_cond_t,
        pub msg_read: libc::c_int,
        pub msg_write: libc::c_int,
        pub thread: libc::pthread_t,
        pub cmd_poll_source: NativeActivityPollSource,
        pub input_poll_source: NativeActivityPollSource,
        pub running: bool,
        pub state_saved: bool,
        pub destroyed: bool,
        pub redraw_needed: bool,
        pub pending_input_queue: *mut ndk_sys::AInputQueue,
        pub pending_window: *mut ndk_sys::ANativeWindow,
        pub pending_content_rect: ndk_sys::ARect,
    }
}

impl AndroidApp {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<ffi::NativeActivityGlue>) -> AndroidApp {
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
    ptr: NonNull<ffi::NativeActivityGlue>,
}
impl Deref for NativeAppGlue {
    type Target = NonNull<ffi::NativeActivityGlue>;

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
                ndk_sys::ALOOPER_POLL_WAKE => {
                    trace!("ALooper_pollAll returned POLL_WAKE");
                    callback(PollEvent::Wake);
                }
                ndk_sys::ALOOPER_POLL_CALLBACK => {
                    // ALooper_pollAll is documented to handle all callback sources internally so it should
                    // never return a _CALLBACK source id...
                    error!("Spurious ALOOPER_POLL_CALLBACK from ALopper_pollAll() (ignored)");
                }
                ndk_sys::ALOOPER_POLL_TIMEOUT => {
                    trace!("ALooper_pollAll returned POLL_TIMEOUT");
                    callback(PollEvent::Timeout);
                }
                ndk_sys::ALOOPER_POLL_ERROR => {
                    // If we have an IO error with our pipe to the main Java thread that's surely
                    // not something we can recover from
                    panic!("ALooper_pollAll returned POLL_ERROR");
                }
                id if id >= 0 => {
                    match id as u32 {
                        ffi::LOOPER_ID_MAIN => {
                            trace!("ALooper_pollAll returned ID_MAIN");
                            let source: *mut ffi::NativeActivityPollSource = source.cast();
                            if source != ptr::null_mut() {
                                if let Some(ipc_cmd) = android_app_read_cmd(native_app.as_ptr()) {
                                    let main_cmd = match ipc_cmd {
                                        // We don't forward info about the AInputQueue to apps since it's
                                        // an implementation details that's also not compatible with
                                        // GameActivity
                                        ffi::AppCmd::InputChanged => None,

                                        ffi::AppCmd::InitWindow => Some(MainEvent::InitWindow {}),
                                        ffi::AppCmd::TermWindow => Some(MainEvent::TerminateWindow {}),
                                        ffi::AppCmd::WindowResized => {
                                            Some(MainEvent::WindowResized {})
                                        }
                                        ffi::AppCmd::WindowRedrawNeeded => {
                                            Some(MainEvent::RedrawNeeded {})
                                        }
                                        ffi::AppCmd::ContentRectChanged => {
                                            Some(MainEvent::ContentRectChanged {})
                                        }
                                        ffi::AppCmd::GainedFocus => Some(MainEvent::GainedFocus),
                                        ffi::AppCmd::LostFocus => Some(MainEvent::LostFocus),
                                        ffi::AppCmd::ConfigChanged => {
                                            Some(MainEvent::ConfigChanged {})
                                        }
                                        ffi::AppCmd::LowMemory => Some(MainEvent::LowMemory),
                                        ffi::AppCmd::Start => Some(MainEvent::Start),
                                        ffi::AppCmd::Resume => Some(MainEvent::Resume {
                                            loader: StateLoader { app: &self },
                                        }),
                                        ffi::AppCmd::SaveState => Some(MainEvent::SaveState {
                                            saver: StateSaver { app: &self },
                                        }),
                                        ffi::AppCmd::Pause => Some(MainEvent::Pause),
                                        ffi::AppCmd::Stop => Some(MainEvent::Stop),
                                        ffi::AppCmd::Destroy => Some(MainEvent::Destroy),
                                    };

                                    trace!("Calling android_app_pre_exec_cmd({ipc_cmd:#?})");
                                    android_app_pre_exec_cmd(native_app.as_ptr(), ipc_cmd);

                                    if let Some(main_cmd) = main_cmd {
                                        trace!("Read ID_MAIN command {ipc_cmd:#?} = {main_cmd:#?}");
                                        match main_cmd {
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

                                        trace!("Invoking callback for ID_MAIN command = {main_cmd:?}");
                                        callback(PollEvent::Main(main_cmd));
                                    }

                                    trace!("Calling android_app_post_exec_cmd({ipc_cmd:#?})");
                                    android_app_post_exec_cmd(native_app.as_ptr(), ipc_cmd);
                                }
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
                            android_app_detach_input_queue_looper(native_app.as_ptr());
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
                left: (*app_ptr).content_rect.left,
                right: (*app_ptr).content_rect.right,
                top: (*app_ptr).content_rect.top,
                bottom: (*app_ptr).content_rect.bottom,
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
            ndk_sys::ANativeActivity_setWindowFlags(
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
                ndk_sys::ANATIVEACTIVITY_SHOW_SOFT_INPUT_IMPLICIT
            } else {
                0
            };
            ndk_sys::ANativeActivity_showSoftInput(na as *mut _, flags);
        }
    }

    // TODO: move into a trait
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        let na = self.native_activity();
        unsafe {
            let flags = if hide_implicit_only {
                ndk_sys::ANATIVEACTIVITY_HIDE_SOFT_INPUT_IMPLICIT_ONLY
            } else {
                0
            };
            ndk_sys::ANativeActivity_hideSoftInput(na as *mut _, flags);
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
            if (*app_ptr).input_queue == ptr::null_mut() {
                return;
            }

            // Reattach the input queue to the looper so future input will again deliver an
            // `InputAvailable` event.
            android_app_attach_input_queue_looper(app_ptr);

            let queue = NonNull::new_unchecked((*app_ptr).input_queue);
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


////////////////////////////
// Rust-side event loop
////////////////////////////

unsafe fn free_saved_state(android_app: *mut ffi::NativeActivityGlue) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    if (*android_app).saved_state != ptr::null_mut() {
        libc::free((*android_app).saved_state);
        (*android_app).saved_state = ptr::null_mut();
        (*android_app).saved_state_size = 0;
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe fn android_app_read_cmd(android_app: *mut ffi::NativeActivityGlue) -> Option<ffi::AppCmd> {
    let mut cmd_i: i8 = 0;
    loop {
        match libc::read((*android_app).msg_read, &mut cmd_i as *mut _ as *mut _, 1) {
            1 => {
                let cmd = ffi::AppCmd::try_from(cmd_i);
                return match cmd {
                    Ok(ffi::AppCmd::SaveState) => {
                        free_saved_state(android_app);
                        Some(ffi::AppCmd::SaveState)
                    }
                    Ok(cmd) => Some(cmd),
                    Err(_) => {
                        log::error!("Spurious, unknown NativeActivityGlue cmd: {}", cmd_i);
                        None
                    }
                };
            }
            -1 => {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    log::error!("Failure reading NativeActivityGlue cmd: {}", err);
                    return None;
                }
            }
            count => {
                log::error!("Spurious read of {count} bytes while reading NativeActivityGlue cmd");
                return None;
            }
        }
    }
}

unsafe fn print_cur_config(android_app: *mut ffi::NativeActivityGlue) {
    let mut lang = [0u8; 2];
    ndk_sys::AConfiguration_getLanguage((*android_app).config, lang[..].as_mut_ptr());
    let lang = if lang[0] == 0 {
        "  ".to_owned()
    } else {
        std::str::from_utf8(&lang[..]).unwrap().to_owned()
    };
    let mut country = "  ".to_owned();
    ndk_sys::AConfiguration_getCountry((*android_app).config, country.as_mut_ptr() as *mut _);

    ndk_sys::AConfiguration_getCountry((*android_app).config, country[..].as_mut_ptr());

    log::debug!("Config: mcc={} mnc={} lang={} cnt={} orien={} touch={} dens={} keys={} nav={} keysHid={} navHid={} sdk={} size={} long={} modetype={} modenight={}",
        ndk_sys::AConfiguration_getMcc((*android_app).config),
        ndk_sys::AConfiguration_getMnc((*android_app).config),
        lang,
        country,
        ndk_sys::AConfiguration_getOrientation((*android_app).config),
        ndk_sys::AConfiguration_getTouchscreen((*android_app).config),
        ndk_sys::AConfiguration_getDensity((*android_app).config),
        ndk_sys::AConfiguration_getKeyboard((*android_app).config),
        ndk_sys::AConfiguration_getNavigation((*android_app).config),
        ndk_sys::AConfiguration_getKeysHidden((*android_app).config),
        ndk_sys::AConfiguration_getNavHidden((*android_app).config),
        ndk_sys::AConfiguration_getSdkVersion((*android_app).config),
        ndk_sys::AConfiguration_getScreenSize((*android_app).config),
        ndk_sys::AConfiguration_getScreenLong((*android_app).config),
        ndk_sys::AConfiguration_getUiModeType((*android_app).config),
        ndk_sys::AConfiguration_getUiModeNight((*android_app).config));
}

unsafe fn android_app_attach_input_queue_looper(android_app: *mut ffi::NativeActivityGlue) {
    if (*android_app).input_queue != ptr::null_mut() {
            log::debug!("Attaching input queue to looper");
            ndk_sys::AInputQueue_attachLooper((*android_app).input_queue,
                    (*android_app).looper, ffi::LOOPER_ID_INPUT as libc::c_int, None,
                    &mut (*android_app).input_poll_source as *mut _ as *mut _);
    }
}

unsafe fn android_app_detach_input_queue_looper(android_app: *mut ffi::NativeActivityGlue) {
    if (*android_app).input_queue != ptr::null_mut() {
        log::debug!("Detaching input queue from looper");
        ndk_sys::AInputQueue_detachLooper((*android_app).input_queue);
    }
}

unsafe fn android_app_pre_exec_cmd(android_app: *mut ffi::NativeActivityGlue, cmd: ffi::AppCmd) {
    match cmd {
        ffi::AppCmd::InputChanged => {
            log::debug!("AppCmd::INPUT_CHANGED\n");
            libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
            if (*android_app).input_queue != ptr::null_mut() {
                android_app_detach_input_queue_looper(android_app);
            }
            (*android_app).input_queue = (*android_app).pending_input_queue;
            if (*android_app).input_queue != ptr::null_mut() {
                android_app_attach_input_queue_looper(android_app);
            }
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
            libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
        }
        ffi::AppCmd::InitWindow => {
            log::debug!("AppCmd::INIT_WINDOW");
            libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
            (*android_app).window = (*android_app).pending_window;
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
            libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
        }
        ffi::AppCmd::TermWindow => {
            log::debug!("AppCmd::TERM_WINDOW");
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
        }
        ffi::AppCmd::Resume | ffi::AppCmd::Start | ffi::AppCmd::Pause | ffi::AppCmd::Stop => {
            log::debug!("activityState={:#?}", cmd);
            libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
            (*android_app).activity_state = cmd as i32;
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
            libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
        }
        ffi::AppCmd::ConfigChanged => {
            log::debug!("AppCmd::CONFIG_CHANGED");
            ndk_sys::AConfiguration_fromAssetManager((*android_app).config,
                    (*(*android_app).activity).assetManager);
            print_cur_config(android_app);
        }
        ffi::AppCmd::Destroy => {
            log::debug!("AppCmd::DESTROY");
            (*android_app).destroy_requested = true;
        }
        _ => { }
    }
}

unsafe fn android_app_post_exec_cmd(android_app: *mut ffi::NativeActivityGlue, cmd: ffi::AppCmd) {
    match cmd {
        ffi::AppCmd::TermWindow => {
            log::debug!("AppCmd::TERM_WINDOW");
            libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
            (*android_app).window = ptr::null_mut();
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
            libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
        }
        ffi::AppCmd::SaveState => {
            log::debug!("AppCmd::SAVE_STATE");
            libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
            (*android_app).state_saved = true;
            libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
            libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
        }
        ffi::AppCmd::Resume => {
            free_saved_state(android_app);
        }
        _ => { }
    }
}

unsafe fn android_app_destroy(android_app: *mut ffi::NativeActivityGlue) {
    log::debug!("android_app_destroy!");
    free_saved_state(android_app);
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    if (*android_app).input_queue != ptr::null_mut() {
        ndk_sys::AInputQueue_detachLooper((*android_app).input_queue);
    }
    ndk_sys::AConfiguration_delete((*android_app).config);
    (*android_app).destroyed = true;
    libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
    // Can't touch android_app object after this.
}

extern "C" fn android_app_main(arg: *mut libc::c_void) -> *mut libc::c_void {
    unsafe {
        let android_app: *mut ffi::NativeActivityGlue = arg.cast();

        (*android_app).config = ndk_sys::AConfiguration_new();
        ndk_sys::AConfiguration_fromAssetManager((*android_app).config, (*(*android_app).activity).assetManager);

        print_cur_config(android_app);

        (*android_app).cmd_poll_source.id = ffi::LOOPER_ID_MAIN as i32;
        (*android_app).cmd_poll_source.app = android_app;
        (*android_app).cmd_poll_source.process = None;

        let looper = ndk_sys::ALooper_prepare(ndk_sys::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as libc::c_int);
        ndk_sys::ALooper_addFd(looper, (*android_app).msg_read, ffi::LOOPER_ID_MAIN as libc::c_int, ndk_sys::ALOOPER_EVENT_INPUT as libc::c_int, None,
                &mut (*android_app).cmd_poll_source as *mut _ as *mut _);
        (*android_app).looper = looper;

        libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
        (*android_app).running = true;
        libc::pthread_cond_broadcast(&mut (*android_app).cond as *mut _);
        libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

        _rust_glue_entry(android_app);

        android_app_destroy(android_app);

        ptr::null_mut()
    }
}


///////////////////////////////
// Java-side callback handling
///////////////////////////////


unsafe fn android_app_create(activity: *mut ndk_sys::ANativeActivity,
    saved_state_in: *const libc::c_void, saved_state_size: libc::size_t) -> *mut ffi::NativeActivityGlue
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

    let android_app = Box::into_raw(Box::new(ffi::NativeActivityGlue {
        activity,
        config: ptr::null_mut(),
        saved_state,
        saved_state_size,
        looper: ptr::null_mut(),
        input_queue: ptr::null_mut(),
        window: ptr::null_mut(),
        content_rect: Rect::empty().into(),
        activity_state: 0,
        destroy_requested: false,
        mutex: libc::PTHREAD_MUTEX_INITIALIZER,
        cond: libc::PTHREAD_COND_INITIALIZER,
        msg_read: msgpipe[0],
        msg_write: msgpipe[1],
        thread: 0,
        cmd_poll_source: ffi::NativeActivityPollSource { id: 0, app: ptr::null_mut(), process: None },
        input_poll_source: ffi::NativeActivityPollSource { id: 0, app: ptr::null_mut(), process: None },
        running: false,
        state_saved: false,
        destroyed: false,
        redraw_needed: false,
        pending_input_queue: ptr::null_mut(),
        pending_window: ptr::null_mut(),
        pending_content_rect: Rect::empty().into(),
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
    while (*android_app).running == false {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    android_app
}

unsafe fn android_app_drop(android_app: *mut ffi::NativeActivityGlue) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    android_app_write_cmd(android_app, ffi::AppCmd::Destroy as i8);
    while !(*android_app).destroyed == false {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    libc::close((*android_app).msg_read);
    libc::close((*android_app).msg_write);
    libc::pthread_cond_destroy(&mut (*android_app).cond as *mut _);
    libc::pthread_mutex_destroy(&mut (*android_app).mutex as *mut _);

    let _android_app = Box::from_raw(android_app);
    // Box dropped here
}

unsafe fn android_app_write_cmd(android_app: *mut ffi::NativeActivityGlue, cmd: i8) {
    loop {
        match libc::write((*android_app).msg_write, &cmd as *const _ as *const _, 1) {
            1 => break,
            -1 => {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    log::error!("Failure writing NativeActivityGlue cmd: {}", err);
                    return;
                }
            }
            count => {
                log::error!("Spurious write of {count} bytes while writing NativeActivityGlue cmd");
                return;
            }
        }
    }
}

unsafe fn android_app_set_input(android_app: *mut ffi::NativeActivityGlue, input_queue: *mut ndk_sys::AInputQueue) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    (*android_app).pending_input_queue = input_queue;
    android_app_write_cmd(android_app, ffi::AppCmd::InputChanged as i8);
    while (*android_app).input_queue != (*android_app).pending_input_queue {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe fn android_app_set_window(android_app: *mut ffi::NativeActivityGlue, window: *mut ndk_sys::ANativeWindow) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    if (*android_app).pending_window != ptr::null_mut() {
        android_app_write_cmd(android_app, ffi::AppCmd::TermWindow as i8);
    }
    (*android_app).pending_window = window;
    if window != ptr::null_mut() {
        android_app_write_cmd(android_app, ffi::AppCmd::InitWindow as i8);
    }
    while (*android_app).window != (*android_app).pending_window {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe fn android_app_set_activity_state(android_app: *mut ffi::NativeActivityGlue, cmd: i8) {
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    android_app_write_cmd(android_app, cmd);
    while (*android_app).activity_state as i8 != cmd {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }
    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);
}

unsafe extern "C" fn on_destroy(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("Destroy: {:p}\n", activity);

    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    (*activity).instance = ptr::null_mut();
    android_app_drop(android_app);
}

unsafe extern "C" fn on_start(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("Start: {:p}\n", activity);

    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::AppCmd::Start as i8);
}

unsafe extern "C" fn on_resume(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("Resume: {:p}\n", activity);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::AppCmd::Resume as i8);
}

unsafe extern "C" fn on_save_instance_state(activity: *mut ndk_sys::ANativeActivity, out_len: *mut ndk_sys::size_t) -> *mut libc::c_void {
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    let mut saved_state: *mut libc::c_void = ptr::null_mut();

    log::debug!("SaveInstanceState: {:p}\n", activity);
    libc::pthread_mutex_lock(&mut (*android_app).mutex as *mut _);
    (*android_app).state_saved = false;
    android_app_write_cmd(android_app, ffi::AppCmd::SaveState as i8);
    while (*android_app).state_saved == false {
        libc::pthread_cond_wait(&mut (*android_app).cond as *mut _, &mut (*android_app).mutex as *mut _);
    }

    if (*android_app).saved_state != ptr::null_mut() {
        saved_state = (*android_app).saved_state;
        *out_len = (*android_app).saved_state_size as _;
        (*android_app).saved_state = ptr::null_mut();
        (*android_app).saved_state_size = 0;
    }

    libc::pthread_mutex_unlock(&mut (*android_app).mutex as *mut _);

    return saved_state;
}

unsafe extern "C" fn on_pause(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("Pause: {:p}\n", activity);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::AppCmd::Pause as i8);
}

unsafe extern "C" fn on_stop(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("Stop: {:p}\n", activity);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_activity_state(android_app, ffi::AppCmd::Stop as i8);
}

unsafe extern "C" fn on_configuration_changed(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("ConfigurationChanged: {:p}\n", activity);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_write_cmd(android_app, ffi::AppCmd::ConfigChanged as i8);
}

unsafe extern "C" fn on_low_memory(activity: *mut ndk_sys::ANativeActivity) {
    log::debug!("LowMemory: {:p}\n", activity);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_write_cmd(android_app, ffi::AppCmd::LowMemory as i8);
}

unsafe extern "C" fn on_window_focus_changed(activity: *mut ndk_sys::ANativeActivity, focused: libc::c_int) {
    log::debug!("WindowFocusChanged: {:p} -- {}\n", activity, focused);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_write_cmd(android_app,
            if focused != 0 { ffi::AppCmd::GainedFocus as i8 } else { ffi::AppCmd::LostFocus as i8});
}

unsafe extern "C" fn on_native_window_created(activity: *mut ndk_sys::ANativeActivity, window: *mut ndk_sys::ANativeWindow) {
    log::debug!("NativeWindowCreated: {:p} -- {:p}\n", activity, window);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_window(android_app, window);
}

unsafe extern "C" fn on_native_window_destroyed(activity: *mut ndk_sys::ANativeActivity, window: *mut ndk_sys::ANativeWindow) {
    log::debug!("NativeWindowDestroyed: {:p} -- {:p}\n", activity, window);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_window(android_app, ptr::null_mut());
}

unsafe extern "C" fn on_input_queue_created(activity: *mut ndk_sys::ANativeActivity, queue: *mut ndk_sys::AInputQueue) {
    log::debug!("InputQueueCreated: {:p} -- {:p}\n", activity, queue);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_input(android_app, queue);
}

unsafe extern "C" fn on_input_queue_destroyed(activity: *mut ndk_sys::ANativeActivity, queue: *mut ndk_sys::AInputQueue) {
    log::debug!("InputQueueDestroyed: {:p} -- {:p}\n", activity, queue);
    let android_app: *mut ffi::NativeActivityGlue = (*activity).instance.cast();
    android_app_set_input(android_app, ptr::null_mut());
}

#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut ndk_sys::ANativeActivity,
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
pub unsafe fn _rust_glue_entry(app: *mut ffi::NativeActivityGlue) {
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
