This crate provides a "glue" layer for building native Rust applications on Android, supporting multiple `Activity` base classes that the application can choose.

Currently the crate supports `NativeActivity` or [`GameActivity`](https://developer.android.com/games/agdk/integrate-game-activity) from the Android Game Development Kit.

This serves a similar purpose to [`android_native_app_glue.c`](https://android.googlesource.com/platform/development/+/4948c163663ecc343c97e4c2a2139234f1d3273f/ndk/sources/android/native_app_glue) for `C/C++` applications.

Here's a minimal illustration of an Android main function and main loop based on this crate:

```rust
use log::info;
use android_activity::{AndroidApp, PollEvent, MainEvent};

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_min_level(log::Level::Info)
    );

    loop {
        app.poll_events(Some(std::time::Duration::from_millis(500)) /* timeout */, |event| {
            match event {
                PollEvent::Wake => { info!("Early wake up"); },
                PollEvent::Timeout => { info!("Timed out"); },
                PollEvent::Main(main_event) => {
                    info!("Main event: {:?}", main_event);
                    match main_event {
                        MainEvent::Destroy => { return; }
                        _ => {}
                    }
                },
                _ => {}
            }

            app.input_events(|event| {
                info!("Input Event: {event:?}");
            });
        });
    }
}
```
