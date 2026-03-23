use std::sync::OnceLock;

use android_activity::{
    input::{InputEvent, KeyAction, KeyEvent, KeyMapChar, MotionAction},
    ndk, ndk_sys, AndroidApp, InputStatus, MainEvent, OnCreateState, PollEvent,
};
use jni::{
    objects::{JObject, JString},
    refs::Global,
    vm::JavaVM,
};
use tracing::{error, info};

jni::bind_java_type! { Context => "android.content.Context" }
jni::bind_java_type! {
    Activity => "android.app.Activity",
    type_map {
        Context => "android.content.Context",
    },
    is_instance_of {
        context: Context
    },
}

jni::bind_java_type! {
    Toast => "android.widget.Toast",
    type_map {
        Context => "android.content.Context",
    },
    methods {
        static fn make_text(context: Context, text: JCharSequence, duration: i32) -> Toast,
        fn show(),
    }
}

// Note: The jni bindings will actually initialize lazily but it can be helpful
// to initialize explicitly to get an up-front error in case there is an issue
// (such as a typo with a method name or incorrect signature) rather than having
// an unpredictable error when using the binding.
fn jni_init(env: &jni::Env) -> jni::errors::Result<()> {
    let _ = ContextAPI::get(env, &Default::default())?;
    let _ = ActivityAPI::get(env, &Default::default())?;
    let _ = ToastAPI::get(env, &Default::default())?;
    // .. call other `get` functions for other bindings here as needed ...
    Ok(())
}

// Called while Activity.onCreate is running
// May be called multiple times if the activity is destroyed and recreated.
#[unsafe(no_mangle)]
fn android_on_create(state: &OnCreateState) {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        use tracing_subscriber::prelude::*;

        unsafe { std::env::set_var("RUST_BACKTRACE", "full") };

        const DEFAULT_ENV_FILTER: &str = "debug,wgpu_hal=info,winit=info,naga=info";
        let filter_layer = tracing_subscriber::EnvFilter::new(DEFAULT_ENV_FILTER);
        let android_layer = paranoid_android::layer(env!("CARGO_PKG_NAME"))
            .with_ansi(false)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_thread_names(true);
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(android_layer)
            .init();
    });

    let vm = unsafe { JavaVM::from_raw(state.vm_as_ptr().cast()) };
    // Note: from here on we can also rely on `JavaVM::singleton()` now that we know it's been initialized.

    let activity = state.activity_as_ptr() as jni::sys::jobject;
    if let Err(err) = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // Initialize JNI bindings
        jni_init(env).expect("Failed to initialize JNI bindings");

        // SAFETY:
        // - The reference / pointer is at least valid until we return
        // - By creating a `Cast` we ensure we can't accidentally delete the reference
        let _activity = unsafe { env.as_cast_raw::<Global<JObject>>(&activity)? };
        // Do something with the activity on the Java main thread, such as call a method or access a field
        Ok(())
    }) {
        error!("Failed to interact with Android SDK on Java main thread: {err:?}");
    }

    eprintln!(
        "android_on_create called on thread {:?}",
        std::thread::current().id()
    );
    info!(
        "android_on_create called on thread {:?}",
        std::thread::current().id()
    );
}

enum ToastDuration {
    Short = 0,
    Long = 1,
}

fn send_toast(outer_app: &AndroidApp, msg: impl AsRef<str>, duration: ToastDuration) {
    let app = outer_app.clone();
    let msg = msg.as_ref().to_string();
    outer_app.run_on_java_main_thread(Box::new(move || {
        // We initialize JavaVM::singleton at the start of `android_main`
        let jvm = jni::JavaVM::singleton().expect("Failed to get singleton JavaVM instance");
        // We use `with_top_local_frame` as a minor optimization because it's guaranteed by
        // `run_on_java_main_thread` that we already have an underlying JNI attachment and local
        // frame. It would also be perfectly reasonable to use `jvm.attach_current_thread()`.
        if let Err(err) = jvm.with_top_local_frame(|env| -> jni::errors::Result<()> {
            let activity: jni::sys::jobject = app.activity_as_ptr() as _;
            let activity = unsafe { env.as_cast_raw::<Global<Activity>>(&activity)? };

            let message = JString::new(env, &msg)?;
            let toast = Toast::make_text(env, activity.as_ref(), &message, duration as i32)?;
            info!("Showing Toast from Rust JNI callback: {msg}");
            toast.show(env)?;

            Ok(())
        }) {
            error!("Failed to execute callback on main thread: {err:?}");
        }
    }));
}

// Called on a dedicated Activity main loop thread, spawned after `android_on_create` returns
// May be called multiple times if the activity is destroyed and recreated.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    eprintln!(
        "android_main started on thread {:?}",
        std::thread::current().id()
    );
    info!(
        "android_main started on thread {:?}",
        std::thread::current().id()
    );

    let mut quit = false;
    let mut redraw_pending = true;
    let mut native_window: Option<ndk::native_window::NativeWindow> = None;

    let mut combining_accent = None;

    send_toast(&app, "Hello from Rust on Android!", ToastDuration::Long);

    while !quit {
        app.poll_events(
            Some(std::time::Duration::from_secs(2)), /* timeout */
            |event| {
                match event {
                    PollEvent::Wake => {
                        info!("Early wake up");
                    }
                    PollEvent::Timeout => {
                        info!("Timed out");
                        // Real app would probably rely on vblank sync via graphics API...
                        redraw_pending = true;
                    }
                    PollEvent::Main(main_event) => {
                        info!("Main event: {:?}", main_event);
                        match main_event {
                            MainEvent::SaveState { saver, .. } => {
                                saver.store("foo://bar".as_bytes());
                            }
                            MainEvent::Pause => {}
                            MainEvent::Resume { loader, .. } => {
                                if let Some(state) = loader.load() {
                                    if let Ok(uri) = String::from_utf8(state) {
                                        info!("Resumed with saved state = {uri:#?}");
                                    }
                                }
                                send_toast(&app, "Resumed!", ToastDuration::Short);
                            }
                            MainEvent::InitWindow { .. } => {
                                native_window = app.native_window();
                                redraw_pending = true;
                            }
                            MainEvent::TerminateWindow { .. } => {
                                native_window = None;
                                redraw_pending = false;
                            }
                            MainEvent::WindowResized { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::RedrawNeeded { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::InputAvailable { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::ConfigChanged { .. } => {
                                info!("Config Changed: {:#?}", app.config());
                                send_toast(&app, "Config Changed!", ToastDuration::Short);
                            }
                            MainEvent::LowMemory => {
                                info!("Low Memory Warning");
                                send_toast(&app, "Low Memory!", ToastDuration::Short);
                            }

                            MainEvent::Destroy => quit = true,
                            _ => { /* ... */ }
                        }
                    }
                    _ => {}
                }

                if redraw_pending {
                    if let Some(native_window) = &native_window {
                        redraw_pending = false;

                        // Handle input, via a lending iterator
                        match app.input_events_iter() {
                            Ok(mut iter) => loop {
                                info!("Checking for next input event...");
                                if !iter.next(|event| {
                                    match event {
                                        InputEvent::KeyEvent(key_event) => {
                                            let combined_key_char = character_map_and_combine_key(
                                                &app,
                                                key_event,
                                                &mut combining_accent,
                                            );
                                            info!("KeyEvent: combined key: {combined_key_char:?}")
                                        }
                                        InputEvent::MotionEvent(motion_event) => {
                                            println!("action = {:?}", motion_event.action());
                                            match motion_event.action() {
                                                MotionAction::Up => {
                                                    let pointer = motion_event.pointer_index();
                                                    let pointer =
                                                        motion_event.pointer_at_index(pointer);
                                                    let x = pointer.x();
                                                    let y = pointer.y();

                                                    println!("POINTER UP {x}, {y}");

                                                    if x < 500.0 && y < 500.0 {
                                                        println!("Requesting to show keyboard");
                                                        send_toast(
                                                            &app,
                                                            "Requesting to show keyboard",
                                                            ToastDuration::Short,
                                                        );
                                                        app.show_soft_input(true);
                                                    } else if x >= 500.0 && y < 500.0 {
                                                        println!("Requesting to hide keyboard");
                                                        send_toast(
                                                            &app,
                                                            "Requesting to hide keyboard",
                                                            ToastDuration::Short,
                                                        );
                                                        app.hide_soft_input(false);
                                                    } else {
                                                        send_toast(
                                                            &app,
                                                            format!("POINTER UP {x}, {y}"),
                                                            ToastDuration::Short,
                                                        );
                                                    }
                                                }
                                                _ => {}
                                            }
                                            let num_pointers = motion_event.pointer_count();
                                            for i in 0..num_pointers {
                                                let pointer = motion_event.pointer_at_index(i);

                                                println!(
                                                    "Pointer[{i}]: id={}, time={}, x={}, y={}",
                                                    pointer.pointer_id(),
                                                    motion_event.event_time(),
                                                    pointer.x(),
                                                    pointer.y(),
                                                );
                                                for sample in pointer.history() {
                                                    println!(
                                                        "  History[{}]: x={}, y={}, time={:?}",
                                                        sample.history_index(),
                                                        sample.x(),
                                                        sample.y(),
                                                        sample.event_time()
                                                    );
                                                }
                                            }
                                        }
                                        InputEvent::TextEvent(state) => {
                                            info!("Input Method State: {state:?}");
                                        }
                                        _ => {}
                                    }

                                    info!("Input Event: {event:?}");
                                    app.run_on_java_main_thread(Box::new(move || {
                                        println!(
                                            "Callback on main thread {:?}",
                                            std::thread::current().id()
                                        );
                                        info!(
                                            "Callback on main thread {:?}",
                                            std::thread::current().id()
                                        );
                                    }));
                                    InputStatus::Unhandled
                                }) {
                                    info!("No more input available");
                                    break;
                                }
                            },
                            Err(err) => {
                                error!("Failed to get input events iterator: {err:?}");
                            }
                        }

                        info!("Render...");
                        dummy_render(native_window);
                    }
                }
            },
        );
    }
}

/// Tries to map the `key_event` to a `KeyMapChar` containing a unicode character or dead key accent
///
/// This shows how to take a `KeyEvent` and look up its corresponding `KeyCharacterMap` and
/// use that to try and map the `key_code` + `meta_state` to a unicode character or a
/// dead key that be combined with the next key press.
fn character_map_and_combine_key(
    app: &AndroidApp,
    key_event: &KeyEvent,
    combining_accent: &mut Option<char>,
) -> Option<KeyMapChar> {
    let device_id = key_event.device_id();

    let key_map = match app.device_key_character_map(device_id) {
        Ok(key_map) => key_map,
        Err(err) => {
            error!("Failed to look up `KeyCharacterMap` for device {device_id}: {err:?}");
            return None;
        }
    };

    match key_map.get(key_event.key_code(), key_event.meta_state()) {
        Ok(KeyMapChar::Unicode(unicode)) => {
            // Only do dead key combining on key down
            if key_event.action() == KeyAction::Down {
                let combined_unicode = if let Some(accent) = combining_accent {
                    match key_map.get_dead_char(*accent, unicode) {
                        Ok(Some(key)) => {
                            info!(
                                "KeyEvent: Combined '{unicode}' with accent '{accent}' to give '{key}'"
                            );
                            Some(key)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            error!(
                                "KeyEvent: Failed to combine 'dead key' accent '{accent}' with '{unicode}': {err:?}"
                            );
                            None
                        }
                    }
                } else {
                    info!("KeyEvent: Pressed '{unicode}'");
                    Some(unicode)
                };
                *combining_accent = None;
                combined_unicode.map(|unicode| KeyMapChar::Unicode(unicode))
            } else {
                Some(KeyMapChar::Unicode(unicode))
            }
        }
        Ok(KeyMapChar::CombiningAccent(accent)) => {
            if key_event.action() == KeyAction::Down {
                info!("KeyEvent: Pressed 'dead key' combining accent '{accent}'");
                *combining_accent = Some(accent);
            }
            Some(KeyMapChar::CombiningAccent(accent))
        }
        Ok(KeyMapChar::None) => {
            // Leave any combining_accent state in tact (seems to match how other
            // Android apps work)
            info!("KeyEvent: Pressed non-unicode key");
            None
        }
        Err(err) => {
            error!("KeyEvent: Failed to get key map character: {err:?}");
            *combining_accent = None;
            None
        }
    }
}

/// Post a NOP frame to the window
///
/// Since this is a bare minimum test app we don't depend
/// on any GPU graphics APIs but we do need to at least
/// convince Android that we're drawing something and are
/// responsive, otherwise it will stop delivering input
/// events to us.
fn dummy_render(native_window: &ndk::native_window::NativeWindow) {
    unsafe {
        let mut buf: ndk_sys::ANativeWindow_Buffer = std::mem::zeroed();
        let mut rect: ndk_sys::ARect = std::mem::zeroed();
        ndk_sys::ANativeWindow_lock(
            native_window.ptr().as_ptr() as _,
            &mut buf as _,
            &mut rect as _,
        );
        // Note: we don't try and touch the buffer since that
        // also requires us to handle various buffer formats
        ndk_sys::ANativeWindow_unlockAndPost(native_window.ptr().as_ptr() as _);
    }
}
