#![cfg(any(feature = "native-activity", doc))]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::ptr;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use libc::c_void;
use log::{error, trace};
use ndk::input_queue::InputQueue;
use ndk::{asset::AssetManager, native_window::NativeWindow};

use crate::error::InternalResult;
use crate::input::{Axis, KeyCharacterMap, KeyCharacterMapBinding};
use crate::input::{TextInputState, TextSpan};
use crate::jni_utils::{self, CloneJavaVM};
use crate::{
    util, AndroidApp, ConfigurationRef, InputStatus, MainEvent, PollEvent, Rect, WindowManagerFlags,
};

pub mod input;

mod glue;
use self::glue::NativeActivityGlue;

pub const LOOPER_ID_MAIN: libc::c_int = 1;
pub const LOOPER_ID_INPUT: libc::c_int = 2;
//pub const LOOPER_ID_USER: ::std::os::raw::c_uint = 3;

/// An interface for saving application state during [MainEvent::SaveState] events
///
/// This interface is only available temporarily while handling a [MainEvent::SaveState] event.
#[derive(Debug)]
pub struct StateSaver<'a> {
    app: &'a AndroidAppInner,
}

impl<'a> StateSaver<'a> {
    /// Stores the given `state` such that it will be available to load the next
    /// time that the application resumes.
    pub fn store(&self, state: &'a [u8]) {
        self.app.native_activity.set_saved_state(state);
    }
}

/// An interface for loading application state during [MainEvent::Resume] events
///
/// This interface is only available temporarily while handling a [MainEvent::Resume] event.
#[derive(Debug)]
pub struct StateLoader<'a> {
    app: &'a AndroidAppInner,
}
impl<'a> StateLoader<'a> {
    /// Returns whatever state was saved during the last [MainEvent::SaveState] event or `None`
    pub fn load(&self) -> Option<Vec<u8>> {
        self.app.native_activity.saved_state()
    }
}

/// A means to wake up the main thread while it is blocked waiting for I/O
#[derive(Clone)]
pub struct AndroidAppWaker {
    // The looper pointer is owned by the android_app and effectively
    // has a 'static lifetime, and the ALooper_wake C API is thread
    // safe, so this can be cloned safely and is send + sync safe
    looper: NonNull<ndk_sys::ALooper>,
}
unsafe impl Send for AndroidAppWaker {}
unsafe impl Sync for AndroidAppWaker {}

impl AndroidAppWaker {
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
}

impl AndroidApp {
    pub(crate) fn new(native_activity: NativeActivityGlue, jvm: CloneJavaVM) -> Self {
        let mut env = jvm.get_env().unwrap(); // We attach to the thread before creating the AndroidApp

        let key_map_binding = match KeyCharacterMapBinding::new(&mut env) {
            Ok(b) => b,
            Err(err) => {
                panic!("Failed to create KeyCharacterMap JNI bindings: {err:?}");
            }
        };

        let app = Self {
            inner: Arc::new(RwLock::new(AndroidAppInner {
                jvm,
                native_activity,
                looper: Looper {
                    ptr: ptr::null_mut(),
                },
                key_map_binding: Arc::new(key_map_binding),
                key_maps: Mutex::new(HashMap::new()),
                input_receiver: Mutex::new(None),
            })),
        };

        {
            let mut guard = app.inner.write().unwrap();

            let main_fd = guard.native_activity.cmd_read_fd();
            unsafe {
                guard.looper.ptr = ndk_sys::ALooper_prepare(
                    ndk_sys::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as libc::c_int,
                );
                ndk_sys::ALooper_addFd(
                    guard.looper.ptr,
                    main_fd,
                    LOOPER_ID_MAIN,
                    ndk_sys::ALOOPER_EVENT_INPUT as libc::c_int,
                    None,
                    //&mut guard.cmd_poll_source as *mut _ as *mut _);
                    ptr::null_mut(),
                );
            }
        }

        app
    }
}

#[derive(Debug)]
struct Looper {
    pub ptr: *mut ndk_sys::ALooper,
}
unsafe impl Send for Looper {}
unsafe impl Sync for Looper {}

#[derive(Debug)]
pub(crate) struct AndroidAppInner {
    pub(crate) jvm: CloneJavaVM,

    pub(crate) native_activity: NativeActivityGlue,
    looper: Looper,

    /// Shared JNI bindings for the `KeyCharacterMap` class
    key_map_binding: Arc<KeyCharacterMapBinding>,

    /// A table of `KeyCharacterMap`s per `InputDevice` ID
    /// these are used to be able to map key presses to unicode
    /// characters
    key_maps: Mutex<HashMap<i32, KeyCharacterMap>>,

    /// While an app is reading input events it holds an
    /// InputReceiver reference which we track to ensure
    /// we don't hand out more than one receiver at a time
    input_receiver: Mutex<Option<Weak<InputReceiver>>>,
}

impl AndroidAppInner {
    pub(crate) fn vm_as_ptr(&self) -> *mut c_void {
        unsafe { (*self.native_activity.activity).vm as _ }
    }

    pub(crate) fn activity_as_ptr(&self) -> *mut c_void {
        // "clazz" is a completely bogus name; this is the _instance_ not class pointer
        unsafe { (*self.native_activity.activity).clazz as _ }
    }

    pub(crate) fn native_activity(&self) -> *const ndk_sys::ANativeActivity {
        self.native_activity.activity
    }

    pub(crate) fn looper(&self) -> *mut ndk_sys::ALooper {
        self.looper.ptr
    }

    pub fn native_window(&self) -> Option<NativeWindow> {
        self.native_activity.mutex.lock().unwrap().window.clone()
    }

    pub fn poll_events<F>(&self, timeout: Option<Duration>, mut callback: F)
    where
        F: FnMut(PollEvent<'_>),
    {
        trace!("poll_events");

        unsafe {
            let mut fd: i32 = 0;
            let mut events: i32 = 0;
            let mut source: *mut c_void = ptr::null_mut();

            let timeout_milliseconds = if let Some(timeout) = timeout {
                timeout.as_millis() as i32
            } else {
                -1
            };

            trace!("Calling ALooper_pollAll, timeout = {timeout_milliseconds}");
            assert!(
                !ndk_sys::ALooper_forThread().is_null(),
                "Application tried to poll events from non-main thread"
            );
            let id = ndk_sys::ALooper_pollAll(
                timeout_milliseconds,
                &mut fd,
                &mut events,
                &mut source as *mut *mut c_void,
            );
            trace!("pollAll id = {id}");
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
                    match id {
                        LOOPER_ID_MAIN => {
                            trace!("ALooper_pollAll returned ID_MAIN");
                            if let Some(ipc_cmd) = self.native_activity.read_cmd() {
                                let main_cmd = match ipc_cmd {
                                    // We don't forward info about the AInputQueue to apps since it's
                                    // an implementation details that's also not compatible with
                                    // GameActivity
                                    glue::AppCmd::InputQueueChanged => None,

                                    glue::AppCmd::InitWindow => Some(MainEvent::InitWindow {}),
                                    glue::AppCmd::TermWindow => Some(MainEvent::TerminateWindow {}),
                                    glue::AppCmd::WindowResized => {
                                        Some(MainEvent::WindowResized {})
                                    }
                                    glue::AppCmd::WindowRedrawNeeded => {
                                        Some(MainEvent::RedrawNeeded {})
                                    }
                                    glue::AppCmd::ContentRectChanged => {
                                        Some(MainEvent::ContentRectChanged {})
                                    }
                                    glue::AppCmd::GainedFocus => Some(MainEvent::GainedFocus),
                                    glue::AppCmd::LostFocus => Some(MainEvent::LostFocus),
                                    glue::AppCmd::ConfigChanged => {
                                        Some(MainEvent::ConfigChanged {})
                                    }
                                    glue::AppCmd::LowMemory => Some(MainEvent::LowMemory),
                                    glue::AppCmd::Start => Some(MainEvent::Start),
                                    glue::AppCmd::Resume => Some(MainEvent::Resume {
                                        loader: StateLoader { app: self },
                                    }),
                                    glue::AppCmd::SaveState => Some(MainEvent::SaveState {
                                        saver: StateSaver { app: self },
                                    }),
                                    glue::AppCmd::Pause => Some(MainEvent::Pause),
                                    glue::AppCmd::Stop => Some(MainEvent::Stop),
                                    glue::AppCmd::Destroy => Some(MainEvent::Destroy),
                                };

                                trace!("Calling pre_exec_cmd({ipc_cmd:#?})");
                                self.native_activity.pre_exec_cmd(
                                    ipc_cmd,
                                    self.looper(),
                                    LOOPER_ID_INPUT,
                                );

                                if let Some(main_cmd) = main_cmd {
                                    trace!("Invoking callback for ID_MAIN command = {main_cmd:?}");
                                    callback(PollEvent::Main(main_cmd));
                                }

                                trace!("Calling post_exec_cmd({ipc_cmd:#?})");
                                self.native_activity.post_exec_cmd(ipc_cmd);
                            }
                        }
                        LOOPER_ID_INPUT => {
                            trace!("ALooper_pollAll returned ID_INPUT");

                            // To avoid spamming the application with event loop iterations notifying them of
                            // input events then we only send one `InputAvailable` per iteration of input
                            // handling. We re-attach the looper when the application calls
                            // `AndroidApp::input_events()`
                            self.native_activity.detach_input_queue_from_looper();
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
            // From the application's pov we assume the looper pointer has a static
            // lifetimes and we can safely assume it is never NULL.
            AndroidAppWaker {
                looper: NonNull::new_unchecked(self.looper.ptr),
            }
        }
    }

    pub fn config(&self) -> ConfigurationRef {
        self.native_activity.config()
    }

    pub fn content_rect(&self) -> Rect {
        self.native_activity.content_rect()
    }

    pub fn asset_manager(&self) -> AssetManager {
        unsafe {
            let activity_ptr = self.native_activity.activity;
            let am_ptr = NonNull::new_unchecked((*activity_ptr).assetManager);
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

    // TODO: move into a trait
    pub fn text_input_state(&self) -> TextInputState {
        TextInputState {
            text: String::new(),
            selection: TextSpan { start: 0, end: 0 },
            compose_region: None,
        }
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, _state: TextInputState) {
        // NOP: Unsupported
    }

    pub fn device_key_character_map(&self, device_id: i32) -> InternalResult<KeyCharacterMap> {
        let mut guard = self.key_maps.lock().unwrap();

        let key_map = match guard.entry(device_id) {
            std::collections::hash_map::Entry::Occupied(occupied) => occupied.get().clone(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                let character_map = jni_utils::device_key_character_map(
                    self.jvm.clone(),
                    self.key_map_binding.clone(),
                    device_id,
                )?;
                vacant.insert(character_map.clone());
                character_map
            }
        };

        Ok(key_map)
    }

    pub fn enable_motion_axis(&self, _axis: Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn disable_motion_axis(&self, _axis: Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn input_events_receiver(&self) -> InternalResult<Arc<InputReceiver>> {
        let mut guard = self.input_receiver.lock().unwrap();

        if let Some(receiver) = &*guard {
            if receiver.strong_count() > 0 {
                return Err(crate::error::InternalAppError::InputUnavailable);
            }
        }
        *guard = None;

        // Get the InputQueue for the NativeActivity (if there is one) and also ensure
        // the queue is re-attached to our event Looper (so new input events will again
        // trigger a wake up)
        let queue = self
            .native_activity
            .looper_attached_input_queue(self.looper(), LOOPER_ID_INPUT);

        // Note: we don't treat it as an error if there is no queue, so if applications
        // iterate input before a queue has been created (e.g. before onStart) then
        // it will simply behave like there are no events available currently.
        let receiver = Arc::new(InputReceiver { queue });

        *guard = Some(Arc::downgrade(&receiver));
        Ok(receiver)
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

#[derive(Debug)]
pub(crate) struct InputReceiver {
    queue: Option<InputQueue>,
}

impl<'a> From<Arc<InputReceiver>> for InputIteratorInner<'a> {
    fn from(receiver: Arc<InputReceiver>) -> Self {
        Self {
            receiver,
            _lifetime: PhantomData,
        }
    }
}

pub(crate) struct InputIteratorInner<'a> {
    // Held to maintain exclusive access to buffered input events
    receiver: Arc<InputReceiver>,
    _lifetime: PhantomData<&'a ()>,
}

impl<'a> InputIteratorInner<'a> {
    pub(crate) fn next<F>(&self, callback: F) -> bool
    where
        F: FnOnce(&input::InputEvent) -> InputStatus,
    {
        let Some(queue) = &self.receiver.queue else {
            log::trace!("no queue available for events");
            return false;
        };

        // Note: we basically ignore errors from event() currently. Looking at the source code for
        // Android's InputQueue, the only error that can be returned here is 'WOULD_BLOCK', which we
        // want to just treat as meaning the queue is empty.
        //
        // ref: https://github.com/aosp-mirror/platform_frameworks_base/blob/master/core/jni/android_view_InputQueue.cpp
        //
        if let Ok(Some(ndk_event)) = queue.event() {
            log::trace!("queue: got event: {ndk_event:?}");

            if let Some(ndk_event) = queue.pre_dispatch(ndk_event) {
                let event = match ndk_event {
                    ndk::event::InputEvent::MotionEvent(e) => {
                        input::InputEvent::MotionEvent(input::MotionEvent::new(e))
                    }
                    ndk::event::InputEvent::KeyEvent(e) => {
                        input::InputEvent::KeyEvent(input::KeyEvent::new(e))
                    }
                    _ => todo!("NDK added a new type"),
                };

                // `finish_event` needs to be called for each event otherwise
                // the app would likely get an ANR
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| callback(&event)));

                let ndk_event = match event {
                    input::InputEvent::MotionEvent(e) => {
                        ndk::event::InputEvent::MotionEvent(e.into_ndk_event())
                    }
                    input::InputEvent::KeyEvent(e) => {
                        ndk::event::InputEvent::KeyEvent(e.into_ndk_event())
                    }
                    _ => unreachable!(),
                };

                let handled = match result {
                    Ok(handled) => handled,
                    Err(payload) => {
                        log::error!("Calling `finish_event` after panic in input event handler, to try and avoid being killed via an ANR");
                        queue.finish_event(ndk_event, false);
                        std::panic::resume_unwind(payload);
                    }
                };

                log::trace!("queue: finishing event");
                queue.finish_event(ndk_event, handled == InputStatus::Handled);
            }

            true
        } else {
            log::trace!("queue: no more events");
            false
        }
    }
}
