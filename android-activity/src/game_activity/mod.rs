#![cfg(feature = "game-activity")]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Deref;
use std::panic::catch_unwind;
use std::ptr;
use std::ptr::NonNull;
use std::sync::Weak;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use libc::c_void;
use log::{error, trace};

use jni_sys::*;

use ndk_sys::ALooper_wake;
use ndk_sys::{ALooper, ALooper_pollAll};

use ndk::asset::AssetManager;
use ndk::configuration::Configuration;
use ndk::native_window::NativeWindow;

use crate::error::InternalResult;
use crate::input::{Axis, KeyCharacterMap, KeyCharacterMapBinding};
use crate::jni_utils::{self, CloneJavaVM};
use crate::util::{abort_on_panic, forward_stdio_to_logcat, log_panic, try_get_path_from_ptr};
use crate::{
    AndroidApp, ConfigurationRef, InputStatus, MainEvent, PollEvent, Rect, WindowManagerFlags,
};

mod ffi;

pub mod input;
use crate::input::{TextInputState, TextSpan};
use input::{InputEvent, KeyEvent, MotionEvent};

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
            if !(*app_ptr).savedState.is_null() {
                libc::free((*app_ptr).savedState);
                (*app_ptr).savedState = ptr::null_mut();
                (*app_ptr).savedStateSize = 0;
            }

            let buf = libc::malloc(state.len());
            if buf.is_null() {
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
            if !(*app_ptr).savedState.is_null() && (*app_ptr).savedStateSize > 0 {
                let buf: &mut [u8] = std::slice::from_raw_parts_mut(
                    (*app_ptr).savedState.cast(),
                    (*app_ptr).savedStateSize,
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
    pub(crate) unsafe fn from_ptr(ptr: NonNull<ffi::android_app>, jvm: CloneJavaVM) -> Self {
        let mut env = jvm.get_env().unwrap(); // We attach to the thread before creating the AndroidApp

        let key_map_binding = match KeyCharacterMapBinding::new(&mut env) {
            Ok(b) => b,
            Err(err) => {
                panic!("Failed to create KeyCharacterMap JNI bindings: {err:?}");
            }
        };

        // Note: we don't use from_ptr since we don't own the android_app.config
        // and need to keep in mind that the Drop handler is going to call
        // AConfiguration_delete()
        let config = Configuration::clone_from_ptr(NonNull::new_unchecked((*ptr.as_ptr()).config));

        Self {
            inner: Arc::new(RwLock::new(AndroidAppInner {
                jvm,
                native_app: NativeAppGlue { ptr },
                config: ConfigurationRef::new(config),
                native_window: Default::default(),
                key_map_binding: Arc::new(key_map_binding),
                key_maps: Mutex::new(HashMap::new()),
                input_receiver: Mutex::new(None),
            })),
        }
    }
}

#[derive(Debug, Clone)]
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

impl NativeAppGlue {
    // TODO: move into a trait
    pub fn text_input_state(&self) -> TextInputState {
        unsafe {
            let activity = (*self.as_ptr()).activity;
            let mut out_state = TextInputState {
                text: String::new(),
                selection: TextSpan { start: 0, end: 0 },
                compose_region: None,
            };
            let out_ptr = &mut out_state as *mut TextInputState;

            let app_ptr = self.as_ptr();
            (*app_ptr).textInputState = 0;

            // NEON WARNING:
            //
            // It's not clearly documented but the GameActivity API over the
            // GameTextInput library directly exposes _modified_ UTF8 text
            // from Java so we need to be careful to convert text to and
            // from UTF8
            //
            // GameTextInput also uses a pre-allocated, fixed-sized buffer for
            // the current text state and has shared `currentState_` that
            // appears to have no lock to guard access from multiple threads.
            //
            // There's also no locking at the GameActivity level, so I'm fairly
            // certain that `GameActivity_getTextInputState` isn't thread
            // safe: https://issuetracker.google.com/issues/294112477
            //
            // Overall this is all quite gnarly - and probably a good reminder
            // of why we want to use Rust instead of C/C++.
            ffi::GameActivity_getTextInputState(
                activity,
                Some(AndroidAppInner::map_input_state_to_text_event_callback),
                out_ptr.cast(),
            );

            out_state
        }
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, state: TextInputState) {
        unsafe {
            let activity = (*self.as_ptr()).activity;
            let modified_utf8 = cesu8::to_java_cesu8(&state.text);
            let text_length = modified_utf8.len() as i32;
            let modified_utf8_bytes = modified_utf8.as_ptr();
            let ffi_state = ffi::GameTextInputState {
                text_UTF8: modified_utf8_bytes.cast(), // NB: may be signed or unsigned depending on target
                text_length,
                selection: ffi::GameTextInputSpan {
                    start: state.selection.start as i32,
                    end: state.selection.end as i32,
                },
                composingRegion: match state.compose_region {
                    Some(span) => {
                        // The GameText subclass of InputConnection only has a special case for removing the
                        // compose region if `start == -1` but the docs for `setComposingRegion` imply that
                        // the region should effectively be removed if any empty region is given (unlike for the
                        // selection region, it's not meaningful to maintain an empty compose region)
                        //
                        // We aim for more consistent behaviour by normalizing any empty region into `(-1, -1)`
                        // to remove the compose region.
                        //
                        // NB `setComposingRegion` itself is documented to clamp start/end to the text bounds
                        // so apart from this special-case handling in GameText's implementation of
                        // `setComposingRegion` then there's nothing special about `(-1, -1)` - it's just an empty
                        // region that should get clamped to `(0, 0)` and then get removed.
                        if span.start == span.end {
                            ffi::GameTextInputSpan { start: -1, end: -1 }
                        } else {
                            ffi::GameTextInputSpan {
                                start: span.start as i32,
                                end: span.end as i32,
                            }
                        }
                    }
                    None => ffi::GameTextInputSpan { start: -1, end: -1 },
                },
            };
            ffi::GameActivity_setTextInputState(activity, &ffi_state as *const _);
        }
    }
}

#[derive(Debug)]
pub struct AndroidAppInner {
    pub(crate) jvm: CloneJavaVM,
    native_app: NativeAppGlue,
    config: ConfigurationRef,
    native_window: RwLock<Option<NativeWindow>>,

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
    pub fn vm_as_ptr(&self) -> *mut c_void {
        let app_ptr = self.native_app.as_ptr();
        unsafe { (*(*app_ptr).activity).vm as _ }
    }

    pub fn activity_as_ptr(&self) -> *mut c_void {
        let app_ptr = self.native_app.as_ptr();
        unsafe { (*(*app_ptr).activity).javaGameActivity as _ }
    }

    pub fn native_window(&self) -> Option<NativeWindow> {
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
            trace!("Calling ALooper_pollAll, timeout = {timeout_milliseconds}");
            let id = ALooper_pollAll(
                timeout_milliseconds,
                &mut fd,
                &mut events,
                &mut source as *mut *mut core::ffi::c_void,
            );
            match id {
                ffi::ALOOPER_POLL_WAKE => {
                    trace!("ALooper_pollAll returned POLL_WAKE");

                    if ffi::android_app_input_available_wake_up(native_app.as_ptr()) {
                        log::debug!("Notifying Input Available");
                        callback(PollEvent::Main(MainEvent::InputAvailable));
                    }

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
                        ffi::NativeAppGlueLooperId_LOOPER_ID_MAIN => {
                            trace!("ALooper_pollAll returned ID_MAIN");
                            let source: *mut ffi::android_poll_source = source.cast();
                            if !source.is_null() {
                                let cmd_i = ffi::android_app_read_cmd(native_app.as_ptr());

                                let cmd = match cmd_i as u32 {
                                    //NativeAppGlueAppCmd_UNUSED_APP_CMD_INPUT_CHANGED => AndroidAppMainEvent::InputChanged,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_INIT_WINDOW => {
                                        MainEvent::InitWindow {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_TERM_WINDOW => {
                                        MainEvent::TerminateWindow {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_RESIZED => {
                                        MainEvent::WindowResized {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_REDRAW_NEEDED => {
                                        MainEvent::RedrawNeeded {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_CONTENT_RECT_CHANGED => {
                                        MainEvent::ContentRectChanged {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_GAINED_FOCUS => {
                                        MainEvent::GainedFocus
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_LOST_FOCUS => {
                                        MainEvent::LostFocus
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_CONFIG_CHANGED => {
                                        MainEvent::ConfigChanged {}
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_LOW_MEMORY => {
                                        MainEvent::LowMemory
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_START => MainEvent::Start,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_RESUME => MainEvent::Resume {
                                        loader: StateLoader { app: self },
                                    },
                                    ffi::NativeAppGlueAppCmd_APP_CMD_SAVE_STATE => {
                                        MainEvent::SaveState {
                                            saver: StateSaver { app: self },
                                        }
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_PAUSE => MainEvent::Pause,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_STOP => MainEvent::Stop,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_DESTROY => MainEvent::Destroy,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_INSETS_CHANGED => {
                                        MainEvent::InsetsChanged {}
                                    }
                                    _ => unreachable!(),
                                };

                                trace!("Read ID_MAIN command {cmd_i} = {cmd:?}");

                                trace!("Calling android_app_pre_exec_cmd({cmd_i})");
                                ffi::android_app_pre_exec_cmd(native_app.as_ptr(), cmd_i);
                                match cmd {
                                    MainEvent::ConfigChanged { .. } => {
                                        self.config.replace(Configuration::clone_from_ptr(
                                            NonNull::new_unchecked((*native_app.as_ptr()).config),
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

                                trace!("Calling android_app_post_exec_cmd({cmd_i})");
                                ffi::android_app_post_exec_cmd(native_app.as_ptr(), cmd_i);
                            } else {
                                panic!("ALooper_pollAll returned ID_MAIN event with NULL android_poll_source!");
                            }
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

    pub fn set_window_flags(
        &self,
        add_flags: WindowManagerFlags,
        remove_flags: WindowManagerFlags,
    ) {
        unsafe {
            let activity = (*self.native_app.as_ptr()).activity;
            ffi::GameActivity_setWindowFlags(activity, add_flags.bits(), remove_flags.bits())
        }
    }

    // TODO: move into a trait
    pub fn show_soft_input(&self, show_implicit: bool) {
        unsafe {
            let activity = (*self.native_app.as_ptr()).activity;
            let flags = if show_implicit {
                ffi::ShowImeFlags_SHOW_IMPLICIT
            } else {
                0
            };
            ffi::GameActivity_showSoftInput(activity, flags);
        }
    }

    // TODO: move into a trait
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        unsafe {
            let activity = (*self.native_app.as_ptr()).activity;
            let flags = if hide_implicit_only {
                ffi::HideImeFlags_HIDE_IMPLICIT_ONLY
            } else {
                0
            };
            ffi::GameActivity_hideSoftInput(activity, flags);
        }
    }

    unsafe extern "C" fn map_input_state_to_text_event_callback(
        context: *mut c_void,
        state: *const ffi::GameTextInputState,
    ) {
        // Java uses a modified UTF-8 format, which is a modified cesu8 format
        let out_ptr: *mut TextInputState = context.cast();
        let text_modified_utf8: *const u8 = (*state).text_UTF8.cast();
        let text_modified_utf8 =
            std::slice::from_raw_parts(text_modified_utf8, (*state).text_length as usize);
        match cesu8::from_java_cesu8(text_modified_utf8) {
            Ok(str) => {
                let len = str.len();
                (*out_ptr).text = String::from(str);

                let selection_start = (*state).selection.start.clamp(0, len as i32 + 1);
                let selection_end = (*state).selection.end.clamp(0, len as i32 + 1);
                (*out_ptr).selection = TextSpan {
                    start: selection_start as usize,
                    end: selection_end as usize,
                };
                if (*state).composingRegion.start < 0 || (*state).composingRegion.end < 0 {
                    (*out_ptr).compose_region = None;
                } else {
                    (*out_ptr).compose_region = Some(TextSpan {
                        start: (*state).composingRegion.start as usize,
                        end: (*state).composingRegion.end as usize,
                    });
                }
            }
            Err(err) => {
                log::error!("Invalid UTF8 text in TextEvent: {}", err);
            }
        }
    }

    // TODO: move into a trait
    pub fn text_input_state(&self) -> TextInputState {
        self.native_app.text_input_state()
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, state: TextInputState) {
        self.native_app.set_text_input_state(state);
    }

    pub(crate) fn device_key_character_map(
        &self,
        device_id: i32,
    ) -> InternalResult<KeyCharacterMap> {
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

    pub fn enable_motion_axis(&mut self, axis: Axis) {
        let axis: u32 = axis.into();
        unsafe { ffi::GameActivityPointerAxes_enableAxis(axis as i32) }
    }

    pub fn disable_motion_axis(&mut self, axis: Axis) {
        let axis: u32 = axis.into();
        unsafe { ffi::GameActivityPointerAxes_disableAxis(axis as i32) }
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

    pub(crate) fn input_events_receiver(&self) -> InternalResult<Arc<InputReceiver>> {
        let mut guard = self.input_receiver.lock().unwrap();

        // Make sure we don't hand out more than one receiver at a time because
        // turning the reciever into an interator will perform a swap_buffers
        // for the buffered input events which shouldn't happen while we're in
        // the middle of iterating events
        if let Some(receiver) = &*guard {
            if receiver.strong_count() > 0 {
                return Err(crate::error::InternalAppError::InputUnavailable);
            }
        }
        *guard = None;

        let receiver = Arc::new(InputReceiver {
            native_app: self.native_app.clone(),
        });

        *guard = Some(Arc::downgrade(&receiver));
        Ok(receiver)
    }

    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            try_get_path_from_ptr((*(*app_ptr).activity).internalDataPath)
        }
    }

    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            try_get_path_from_ptr((*(*app_ptr).activity).externalDataPath)
        }
    }

    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        unsafe {
            let app_ptr = self.native_app.as_ptr();
            try_get_path_from_ptr((*(*app_ptr).activity).obbPath)
        }
    }
}

struct MotionEventsLendingIterator {
    pos: usize,
    count: usize,
}

impl MotionEventsLendingIterator {
    fn new(buffer: &InputBuffer) -> Self {
        Self {
            pos: 0,
            count: buffer.motion_events_count(),
        }
    }
    fn next<'buf>(&mut self, buffer: &'buf InputBuffer) -> Option<MotionEvent<'buf>> {
        if self.pos < self.count {
            // Safety:
            // - This iterator currently has exclusive access to the front buffer of events
            // - We know the buffer is non-null
            // - `pos` is less than the number of events stored in the buffer
            let ga_event = unsafe {
                (*buffer.ptr.as_ptr())
                    .motionEvents
                    .add(self.pos)
                    .as_ref()
                    .unwrap()
            };
            let event = MotionEvent::new(ga_event);
            self.pos += 1;
            Some(event)
        } else {
            None
        }
    }
}

struct KeyEventsLendingIterator {
    pos: usize,
    count: usize,
}

impl KeyEventsLendingIterator {
    fn new(buffer: &InputBuffer) -> Self {
        Self {
            pos: 0,
            count: buffer.key_events_count(),
        }
    }
    fn next<'buf>(&mut self, buffer: &'buf InputBuffer) -> Option<KeyEvent<'buf>> {
        if self.pos < self.count {
            // Safety:
            // - This iterator currently has exclusive access to the front buffer of events
            // - We know the buffer is non-null
            // - `pos` is less than the number of events stored in the buffer
            let ga_event = unsafe {
                (*buffer.ptr.as_ptr())
                    .keyEvents
                    .add(self.pos)
                    .as_ref()
                    .unwrap()
            };
            let event = KeyEvent::new(ga_event);
            self.pos += 1;
            Some(event)
        } else {
            None
        }
    }
}

struct InputBuffer<'a> {
    ptr: NonNull<ffi::android_input_buffer>,
    _lifetime: PhantomData<&'a ffi::android_input_buffer>,
}

impl<'a> InputBuffer<'a> {
    pub(crate) fn from_ptr(ptr: NonNull<ffi::android_input_buffer>) -> InputBuffer<'a> {
        Self {
            ptr,
            _lifetime: PhantomData,
        }
    }

    pub fn motion_events_count(&self) -> usize {
        unsafe { (*self.ptr.as_ptr()).motionEventsCount as usize }
    }

    pub fn key_events_count(&self) -> usize {
        unsafe { (*self.ptr.as_ptr()).keyEventsCount as usize }
    }
}

impl<'a> Drop for InputBuffer<'a> {
    fn drop(&mut self) {
        unsafe {
            ffi::android_app_clear_motion_events(self.ptr.as_ptr());
            ffi::android_app_clear_key_events(self.ptr.as_ptr());
        }
    }
}

/// Conceptually we can think of this like the receiver end of an
/// input events channel.
///
/// After being passed back to AndroidApp it gets turned into a
/// lending iterator for pending input events.
///
/// It serves two purposes:
/// 1. It represents an exclusive access to input events (the application
///    can only have one receiver at a time) and it's intended to support
///    the double-buffering design for input events in GameActivity where
///    we issue a swap_buffers before iterating events and wouldn't want
///    another swap to be possible before finishing - especially since
///    we want to borrow directly from the buffer while dispatching.
/// 2. It doesn't borrow from AndroidAppInner so we can pass it back to
///    AndroidApp which can drop its lock around AndroidAppInner and
///    it can then be turned into a lending iterator. (We wouldn't
///    be able to pass the iterator back to the application if it
///    borrowed from within the lock and we need to drop the lock
///    because otherwise the app wouldn't be able to access the AndroidApp
///    API in any way while iterating events)
#[derive(Debug)]
pub(crate) struct InputReceiver {
    // Safety: the native_app effectively has a static lifetime and it
    // has its own internal locking when calling
    // `android_app_swap_input_buffers`
    native_app: NativeAppGlue,
}

impl<'a> From<Arc<InputReceiver>> for InputIteratorInner<'a> {
    fn from(receiver: Arc<InputReceiver>) -> Self {
        let buffered = unsafe {
            let app_ptr = receiver.native_app.as_ptr();
            let input_buffer = ffi::android_app_swap_input_buffers(app_ptr);
            NonNull::new(input_buffer).map(|input_buffer| {
                let buffer = InputBuffer::from_ptr(input_buffer);
                let keys_iter = KeyEventsLendingIterator::new(&buffer);
                let motion_iter = MotionEventsLendingIterator::new(&buffer);
                BufferedEvents::<'a> {
                    buffer,
                    keys_iter,
                    motion_iter,
                }
            })
        };

        let native_app = receiver.native_app.clone();
        Self {
            _receiver: receiver,
            buffered,
            native_app,
            text_event_checked: false,
        }
    }
}

struct BufferedEvents<'a> {
    buffer: InputBuffer<'a>,
    keys_iter: KeyEventsLendingIterator,
    motion_iter: MotionEventsLendingIterator,
}

pub(crate) struct InputIteratorInner<'a> {
    // Held to maintain exclusive access to buffered input events
    _receiver: Arc<InputReceiver>,

    buffered: Option<BufferedEvents<'a>>,
    native_app: NativeAppGlue,
    text_event_checked: bool,
}

impl<'a> InputIteratorInner<'a> {
    pub(crate) fn next<F>(&mut self, callback: F) -> bool
    where
        F: FnOnce(&input::InputEvent) -> InputStatus,
    {
        if let Some(buffered) = &mut self.buffered {
            if let Some(key_event) = buffered.keys_iter.next(&buffered.buffer) {
                let _ = callback(&InputEvent::KeyEvent(key_event));
                return true;
            }
            if let Some(motion_event) = buffered.motion_iter.next(&buffered.buffer) {
                let _ = callback(&InputEvent::MotionEvent(motion_event));
                return true;
            }
            self.buffered = None;
        }

        if !self.text_event_checked {
            self.text_event_checked = true;
            unsafe {
                let app_ptr = self.native_app.as_ptr();

                // XXX: It looks like the GameActivity implementation should
                // be using atomic ops to set this flag, and require us to
                // use atomics to check and clear it too.
                //
                // We currently just hope that with the lack of atomic ops that
                // the compiler isn't reordering code so this gets flagged
                // before the java main thread really updates the state.
                if (*app_ptr).textInputState != 0 {
                    let state = self.native_app.text_input_state(); // Will clear .textInputState
                    let _ = callback(&InputEvent::TextEvent(state));
                    return true;
                }
            }
        }
        false
    }
}

// Rust doesn't give us a clean way to directly export symbols from C/C++
// so we rename the C/C++ symbols and re-export these JNI entrypoints from
// Rust...
//
// https://github.com/rust-lang/rfcs/issues/2771
extern "C" {
    pub fn Java_com_google_androidgamesdk_GameActivity_initializeNativeCode_C(
        env: *mut JNIEnv,
        javaGameActivity: jobject,
        internalDataDir: jstring,
        obbDir: jstring,
        externalDataDir: jstring,
        jAssetMgr: jobject,
        savedState: jbyteArray,
        javaConfig: jobject,
    ) -> jlong;

    pub fn GameActivity_onCreate_C(
        activity: *mut ffi::GameActivity,
        savedState: *mut ::std::os::raw::c_void,
        savedStateSize: libc::size_t,
    );
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_google_androidgamesdk_GameActivity_initializeNativeCode(
    env: *mut JNIEnv,
    java_game_activity: jobject,
    internal_data_dir: jstring,
    obb_dir: jstring,
    external_data_dir: jstring,
    jasset_mgr: jobject,
    saved_state: jbyteArray,
    java_config: jobject,
) -> jni_sys::jlong {
    Java_com_google_androidgamesdk_GameActivity_initializeNativeCode_C(
        env,
        java_game_activity,
        internal_data_dir,
        obb_dir,
        external_data_dir,
        jasset_mgr,
        saved_state,
        java_config,
    )
}

#[no_mangle]
pub unsafe extern "C" fn GameActivity_onCreate(
    activity: *mut ffi::GameActivity,
    saved_state: *mut ::std::os::raw::c_void,
    saved_state_size: libc::size_t,
) {
    GameActivity_onCreate_C(activity, saved_state, saved_state_size);
}

extern "Rust" {
    pub fn android_main(app: AndroidApp);
}

// This is a spring board between android_native_app_glue and the user's
// `app_main` function. This is run on a dedicated thread spawned
// by android_native_app_glue.
#[no_mangle]
pub unsafe extern "C" fn _rust_glue_entry(native_app: *mut ffi::android_app) {
    abort_on_panic(|| {
        let _join_log_forwarder = forward_stdio_to_logcat();

        let jvm = unsafe {
            let jvm = (*(*native_app).activity).vm;
            let activity: jobject = (*(*native_app).activity).javaGameActivity;
            ndk_context::initialize_android_context(jvm.cast(), activity.cast());

            let jvm = CloneJavaVM::from_raw(jvm).unwrap();
            // Since this is a newly spawned thread then the JVM hasn't been attached
            // to the thread yet. Attach before calling the applications main function
            // so they can safely make JNI calls
            jvm.attach_current_thread_permanently().unwrap();
            jvm
        };

        unsafe {
            // Name thread - this needs to happen here after attaching to a JVM thread,
            // since that changes the thread name to something like "Thread-2".
            let thread_name = std::ffi::CStr::from_bytes_with_nul(b"android_main\0").unwrap();
            libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());

            let app = AndroidApp::from_ptr(NonNull::new(native_app).unwrap(), jvm.clone());

            // We want to specifically catch any panic from the application's android_main
            // so we can finish + destroy the Activity gracefully via the JVM
            catch_unwind(|| {
                // XXX: If we were in control of the Java Activity subclass then
                // we could potentially run the android_main function via a Java native method
                // springboard (e.g. call an Activity subclass method that calls a jni native
                // method that then just calls android_main()) that would make sure there was
                // a Java frame at the base of our call stack which would then be recognised
                // when calling FindClass to lookup a suitable classLoader, instead of
                // defaulting to the system loader. Without this then it's difficult for native
                // code to look up non-standard Java classes.
                android_main(app);
            })
            .unwrap_or_else(|panic| log_panic(panic));

            // Let JVM know that our Activity can be destroyed before detaching from the JVM
            //
            // "Note that this method can be called from any thread; it will send a message
            //  to the main thread of the process where the Java finish call will take place"
            ffi::GameActivity_finish((*native_app).activity);

            // This should detach automatically but lets detach explicitly to avoid depending
            // on the TLS trickery in `jni-rs`
            jvm.detach_current_thread();

            ndk_context::release_android_context();
        }
    })
}
