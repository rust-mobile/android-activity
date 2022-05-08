
use game_activity::{PollEvent, MainEvent};
use log::Level;
use log::trace;
use std::time::Duration;
use serde::{Serialize, Deserialize};


#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    uri: String,
}

#[no_mangle]
extern "C" fn android_main() {

    android_logger::init_once(
        android_logger::Config::default().with_min_level(Level::Trace)
    );

    let mut quit = false;
    let mut redraw_pending = true;
    let mut render_state: Option<()> = Default::default();

    let app = game_activity::android_app();
    while !quit {
        app.poll_events(Some(Duration::from_millis(500)) /* timeout */, |event| {
            match event {
                PollEvent::Wake => { trace!("Early wake up"); },
                PollEvent::Timeout => {
                    trace!("Timed out");
                    // Real app would probably rely on vblank sync via graphics API...
                    redraw_pending = true;
                },
                PollEvent::Main(main_event) => {
                    trace!("Main event: {:?}", main_event);
                    match main_event {
                        MainEvent::SaveState { saver, .. } => {
                            let state = serde_json::to_vec(&AppState { uri: format!("foo://bar") }).unwrap();
                            saver.store(&state);
                        },
                        MainEvent::Pause => {},
                        MainEvent::Resume { loader, .. } => {
                            if let Some(state) = loader.load() {
                                let _state: AppState = serde_json::from_slice(&state).unwrap();
                            }
                        },
                        MainEvent::InitWindow { .. } => {
                            render_state = Some(());
                            redraw_pending = true;
                        },
                        MainEvent::TerminateWindow { .. } => {
                            render_state = None;
                        }
                        MainEvent::WindowResized { .. } => { redraw_pending = true; },
                        MainEvent::RedrawNeeded { ..} => { redraw_pending = true; },
                        MainEvent::LowMemory => {},

                        MainEvent::Destroy => { quit = true },
                        _ => { /* ... */}
                    }
                },
                _ => {}
            }

            if redraw_pending {
                if let Some(_rs) = render_state {
                    redraw_pending = false;

                    // Handle input
                    if let Some(buf) = app.swap_input_buffers() {
                        for motion in buf.motion_events_iter() {
                            trace!("Motion Event: {motion:?}")
                        }
                        for key in buf.key_events_iter() {
                            trace!("Key Event: {key:?}")
                        }
                    }

                    // Render...
                }
            }
        });
    }
}
