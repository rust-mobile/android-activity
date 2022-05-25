This project includes a number of Android "glue" crates for native Rust development
(comparable to [`android_native_app_glue.c`][ndk_concepts] which supports C/C++
applications).

These glue crates provide a way to load a `cdylib` via the `onCreate` method of
your `Activity` class; run an `android_main()` function in a separate thread from the Java
main thread and marshal events (such as lifecycle events and input events) between
Java and your native thread.

[ndk_concepts]: https://developer.android.com/ndk/guides/concepts#naa

### Example

```
cargo init --lib --name=example
```

Cargo.toml
```
[dependencies]
log = "0.4"
android_logger = "0.11"
native-activity = { git = "https://github.com/rib/agdk-rust/" }

[lib]
crate_type = ["cdylib"]
```

lib.rs
```rust
use log::info;
use native_activity::{PollEvent, MainEvent};

#[no_mangle]
extern "C" fn android_main() {
    android_logger::init_once(
        android_logger::Config::default().with_min_level(log::Level::Info)
    );

    let app = native_activity::android_app();
    loop {
        app.poll_events(Some(std::time::Duration::from_millis(500)) /* timeout */, |event| {
            match event {
                PollEvent::Wake => { info!("Early wake up"); },
                PollEvent::Timeout => { info!("Hello, World!"); },
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

```
rustup target add aarch64-linux-android
cargo install cargo-apk
cargo apk run
```

# Game Activity

Originally the aim was to enable support for building Rust applications based on the
[GameActivity] based class provided by [Google's Android Game Development Kit][agdk]
which should also facilitate integration with additional AGDK libraries including:
1. [Game Text Input](https://developer.android.com/games/agdk/add-support-for-text-input): a library
to help fullscreen native applications utilize the Android soft keyboard.
2. [Game Controller Library, aka 'Paddleboat'](https://developer.android.com/games/sdk/game-controller):
a native library designed to help support access to game controller inputs.
3.[Frame Pacing Library, aka ' Swappy'](https://developer.android.com/games/sdk/frame-pacing): a library
that helps OpenGL and Vulkan games achieve smooth rendering and correct frame pacing on Android.
3. [Memory Advice API](https://developer.android.com/games/sdk/memory-advice/overview): an API to
help applications monitor their own memory usage to stay within safe limits for the system.
4. [Oboe audio library](https://developer.android.com/games/sdk/oboe): a low-latency audio API for native
applications.

Since `GameActivity` is based on the widely used [AppCompatActivity] base class, it also
provides a variety of back ported Activity APIs which can make it more practical to
support a wider range of devices and Android versions.

[GameActivity]: https://developer.android.com/games/agdk/integrate-game-activity
[agdk]: https://developer.android.com/games/agdk
[AppCompatActivity]: https://developer.android.com/reference/androidx/appcompat/app/AppCompatActivity

# Native Activity

This project also supports [`NativeActivity`][NativeActivity] based applications. Although
NativeActivity is more limited than `GameActivity` and does not derive from `AppCompatActivity` it
can sometimes still be convenient to build on `NativeActivity` in situations where you are using a
limited/minimal build system that is not able to compile Java or Kotlin code or fetch from Maven
repositories. This is because `NativeActivity` is included as part of the Android platform.

[NativeActivity]: https://developer.android.com/reference/android/app/NativeActivity

# Design

## Compatibility

Both the [game-activity] glue crate and the [native-activity] glue crate support a common API that allows
them to be used interchangably, depending on which base class your application decides to use.

Although it's expected that the `game-activity` crate will gain features that aren't possible with `native-activity`
those should be covered by optional features that allow downstream crates, such as Winit to practically be
able to support alternative glue layers.

The hope is that the core, common API can be supported via any Activity subclass that your
application needs to use.

[game-activity]: https://github.com/rib/agdk-rust/tree/main/game-activity
[native-activity]: https://github.com/rib/agdk-rust/tree/main/native-activity

## API Summary


### `android_main` entrypoint
The glue crates define a standard entrypoint ABI for your `cdylib` that looks like:

```rust
#[no_mangle]
extern "C" fn android_main() {
    ...
}
```

There's currently no high-level macro provided for things like initializing logging or allowing the
main function to return a `Result<>` since it's expected that different downstream frameworks may
each have differing oppinions on the details and may want to provide their own macros.

### Global `AndroidApp`

The glue layer provides a `'static` `AndroidApp` API to access state about your running application
and handle synchronized interaction between your native Rust application and the `Activity` running
on the Java main thread.

For example, the `AndroidApp` API enables:
1. Access to Android lifecycle events
2. Notifications of SurfaceView lifecycle events
3. Access to input events
4. Ability to save and restore state each time your process stops and starts

For example:
```rust
#[no_mangle]
extern "C" fn android_main() {
    let app = game_activity::android_app();
    ...
}
```

_Note: that some of the `AndroidApp` APIs (such as for polling events) are only deemed safe to use
from the application's main thread_


### Synchronized event callbacks

The `AndroidApp::poll_events()` API is similar to the Winit `EventLoop::run` API in that it
takes a `FnMut` closure that is called for each outstanding event (such as for lifecycle events).
This is modeled on the original `android_native_app_glue` design for C/C++ that reserves the
ability for the glue layer to insert "pre-" and "-post" hooks around the application's event
callback that can handle any required synchronization with the Java main thread.

For example, when the Java main thread notifies the glue layer that its `SurfaceView` is being
destroyed the Java thread will then block until it gets an explicit acknowledgement that the
native application has had an opportunity to react to this notification. The glue layer will
automatically release the blocked Java thread once it has delivered the corresponding event.

For example:
```rust
use native_activity::{PollEvent, MainEvent};
use log::info;

#[no_mangle]
extern "C" fn android_main() {
    android_logger::init_once(
        android_logger::Config::default().with_min_level(log::Level::Info)
    );

    let mut quit = false;
    let mut redraw_pending = true;
    let mut render_state: Option<()> = Default::default();

    let app = native_activity::android_app();
    while !quit {
        app.poll_events(Some(std::time::Duration::from_millis(500)) /* timeout */, |event| {
            match event {
                PollEvent::Wake => { info!("Early wake up"); },
                PollEvent::Timeout => {
                    info!("Timed out");
                    // Real app would probably rely on vblank sync via graphics API...
                    redraw_pending = true;
                },
                PollEvent::Main(main_event) => {
                    info!("Main event: {:?}", main_event);
                    match main_event {
                        MainEvent::SaveState { saver, .. } => {
                            saver.store("foo://bar".as_bytes());
                        },
                        MainEvent::Pause => {},
                        MainEvent::Resume { loader, .. } => {
                            if let Some(state) = loader.load() {
                                if let Ok(uri) = String::from_utf8(state) {
                                    info!("Resumed with saved state = {uri:#?}");
                                }
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
                    app.input_events(|event| {
                        info!("Input Event: {event:?}");

                    });

                    info!("Render...");
                }
            }
        });
    }
}
```
