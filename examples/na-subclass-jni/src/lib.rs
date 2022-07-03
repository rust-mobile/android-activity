
use android_activity::{PollEvent, MainEvent, AndroidApp};
use log::Level;
use log::{trace, info};
use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    uri: String,
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_min_level(Level::Info)
    );

    let mut quit = false;
    let mut redraw_pending = true;
    let mut render_state: Option<()> = Default::default();

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
                    info!("Main event: {:?}", main_event);
                    match main_event {
                        MainEvent::SaveState { saver, .. } => {
                            let state = serde_json::to_vec(&AppState { uri: format!("foo://bar") }).unwrap();
                            saver.store(&state);
                        },
                        MainEvent::Pause => {},
                        MainEvent::Resume { loader, .. } => {
                            if let Some(state) = loader.load() {
                                let state: AppState = serde_json::from_slice(&state).unwrap();
                                info!("Resumed with saved state = {state:#?}");
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
                info!("Checking input: START");
                if let Some(_rs) = render_state {
                    redraw_pending = false;

                    // Handle input
                    app.input_events(|event| {
                        info!("Input Event: {event:?}");

                    });

                    // Render...
                }
                info!("Checking input: DONE");
            } else {
                info!("No redraw pending");
            }
        });
    }
}


#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_co_realfit_nasubclassjni_MainActivity_notifyOnNewIntent(
    _env: jni::JNIEnv,
    _class: jni::objects::JObject, // This is the JClass, not the instance,
    _activity: jni::objects::JObject,
) {
    info!("onNewIntent was called!");
}