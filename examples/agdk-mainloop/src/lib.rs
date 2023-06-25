use android_activity::{
    input::{InputEvent, KeyAction, KeyEvent, KeyMapChar, MotionAction},
    AndroidApp, InputStatus, MainEvent, PollEvent,
};
use log::info;

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));

    let mut quit = false;
    let mut redraw_pending = true;
    let mut native_window: Option<ndk::native_window::NativeWindow> = None;

    let mut combining_accent = None;

    while !quit {
        app.poll_events(
            Some(std::time::Duration::from_secs(1)), /* timeout */
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
                            }
                            MainEvent::InitWindow { .. } => {
                                native_window = app.native_window();
                                redraw_pending = true;
                            }
                            MainEvent::TerminateWindow { .. } => {
                                native_window = None;
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
                            }
                            MainEvent::LowMemory => {}

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
                                                    if x < 200.0 && y < 200.0 {
                                                        println!("Requesting to show keyboard");
                                                        app.show_soft_input(true);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                        InputEvent::TextEvent(state) => {
                                            info!("Input Method State: {state:?}");
                                        }
                                        _ => {}
                                    }

                                    info!("Input Event: {event:?}");
                                    InputStatus::Unhandled
                                }) {
                                    info!("No more input available");
                                    break;
                                }
                            },
                            Err(err) => {
                                log::error!("Failed to get input events iterator: {err:?}");
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
            log::error!("Failed to look up `KeyCharacterMap` for device {device_id}: {err:?}");
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
                            info!("KeyEvent: Combined '{unicode}' with accent '{accent}' to give '{key}'");
                            Some(key)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            log::error!("KeyEvent: Failed to combine 'dead key' accent '{accent}' with '{unicode}': {err:?}");
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
            log::error!("KeyEvent: Failed to get key map character: {err:?}");
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
