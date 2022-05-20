This crate provides a "glue" layer for building native Rust applications on Android based on the Android Game Development Kit's [`GameActivity`](https://developer.android.com/games/agdk/integrate-game-activity) class.

This serves a similar purpose to [`android_native_app_glue.c`](https://android.googlesource.com/platform/development/+/4948c163663ecc343c97e4c2a2139234f1d3273f/ndk/sources/android/native_app_glue) for `C/C++` applications but instead of being based on the `NativeActivity` class, this is instead based on `GameActivity` that itself is is based on [`AppCompatActivity`](https://developer.android.com/reference/androidx/appcompat/app/AppCompatActivity)

This abstraction builds directly on the native glue layer provided by the [AGDK](https://developer.android.com/games/agdk) project so that it's practical to keep in sync with any upstream fixes.

The general way in which it works internally is to spawn a dedicated thread for the main function of your Rust application and uses IPC via a pipe to marshal events from Java (such as lifecycle events) to the native application.

Here's a minimal illustration of an Android main function and main loop based on this crate _(for portability then real applications would probably use winit which would handle some of this internally)_:

```rust
#[no_mangle]
extern "C" fn android_main() {
    android_logger::init_once(
        Config::default().with_min_level(Level::Trace)
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
```

# Motivations

1. Firstly I'd like to write a cross-platform UI (in egui) to test a Rust Bluetooth library I've been working on.
2. Secondly I saw that Bevy's Android support needs some tlc currently ([#86](https://github.com/bevyengine/bevy/issues/86))
3. I wanted an excuse to poke at some of the backend details for winit and see how it works with wgpu - since I'm a low-level graphics and windowing system person that likes Rust :-D

For the Bluetooth library I notably can't directly use `NativeActivity` since I have to subclass Activity to be able to get callbacks for the result of Intents due to how Android's Companion API works. Theoretically I could subclass `NativeActivity` in Java but for testing so far I have an application that is based on [`AppCompatActivity`](https://developer.android.com/reference/androidx/appcompat/app/AppCompatActivity) which I'm inclined to keep as a base if possible. For example `AppCompatActivity` offers a better solution for receiving Intent results from another activity that doesn't require subclassing the Activity further and this API/solution simply isn't available based on `NativeActivity` or even a custom subclass.

Although I've worked with `NativeActivity` a decent amount in the past I've only recently had to venture into needing more features than NativeActivity can support directly. From seeing how developers tend to create the Activity class for non-native development (I'm not experienced with Java/Kotlin programming on Android) it looks like [`AppCompatActivity`](https://developer.android.com/reference/androidx/appcompat/app/AppCompatActivity) along with [Jetpack](https://developer.android.com/jetpack) libraries offer a much more comprehensive foundation for developing Android applications than being based directly on `Activity`, like `NativeActivity`. I think it stands to reason that native Rust (and hybrid) applications on Android should be able to leverage the same foundations that modern Android apps have - at least as an option for times when `NativeActivity` isn't enough.

I recently discovered Google's [Android Game Development Kit](https://developer.android.com/games/agdk) project which aims to offer a more comprehensive native development experience for game developers. The project provides native bindings that make it easier to handle text input and IME handling, as well as supporting game controllers and other fiddly details that could be particularly useful for game engines like Bevy. One of the components they provide is a [`GameActivity`](https://developer.android.com/games/agdk/integrate-game-activity) class that is essentially a modern alternative to `NativeActivity` that's based on `AppCompatActivity`. This basically just sounded ideal here and so I started looking at whether it would be feasible to create an alternative to `ndk-glue` that would support building Rust apps based on `GameActivity` along with an updated backend for winit.


# Compatibility

The original intent was to copy the API of [ndk-glue](https://github.com/rust-windowing/android-ndk-rs) for the sake of being able to use with [winit](https://github.com/rust-windowing/winit) (unmodified) but as I progressed I found that wasn't going to be realistic / practical.

In particular the way in which the glue layer needs to synchronize with the Java main thread with pre- and post- processing surrounding the handling of lifecycle events led to a different `poll_events()` API.

The existing Android backend for winit also appeared to have a rather complex event polling scheme which I was keen to simplify and make more comparable to the Linux mio/epoll based backend where there would be a very clear, single place where the mainloop would block waiting for events.

When I went further and added support for saving and restoring application state I found that it worked nicely to expose this via a transient `StateSaver` and `StateLoader` that's passed as part of the `SaveState` and `Resume` events, which was also a further change compared to `ndk-glue`.

This crate does integrate with `ndk-context` like `ndk-glue` does so that other crates that just need access to the JVM and/or Activity object from Rust can work in the same way.

Input handling is notably different between `NativeActivity` (which is based on `AInputQueue`) and `GameActivity` that does its own double buffering of key and motion events. As much as possible this crate provides an API that is compatible (the `MotionEvent` and `KeyEvent` APIs are almost identical) but it doesn't track any motion history data automatically and provides a different `swap_input_buffers` API for reading events. As an optimization `GameActivity` minimizes what axis are captured for pointer events and so there are also additional APIs exposed for applications to explicitly opt-in to additional axis values.


# Why not pure Rust?

One of the appeals (imho) of the AGDK project was that they are maintaining native Android libraries that look like they could be generally useful for native Rust apps (especially game engines) and even though they are C/C++ libraries that could theoretically be re-written in pure Rust it seems more worthwhile / practical to first try and leverage these libraries directly before considering whether it makes sense to re-implement any of them.

This crate uses the upstream glue layer written in C/C++ and provides a small Rust API on top for ergonomics. This way it's hopefully practical to keep in sync with upstream changes and bug fixes and to benefit from upstream testing.

The C/C++ GameActivity / android_native_app_glue code isn't _that_ complex and could well be re-implemented in Rust at some point, although it's not clear how beneficial it would be. I would note that `ndk-glue` did go down the route of re-implementing the glue layer for `NativeActivity` purely in Rust but I wasn't confident about it's synchronization model is correct which imho highlights that the code should only be re-written if there are some clear technical benefits. (See here for more discussion: https://github.com/rust-windowing/winit/issues/2293)


# Synchronizing with Upstream...

Upstream distribute `android_native_app_glue.c` and `GameActivity.cpp` code as a "prefab" that is bundled as part of a `GameActivity-release.aar` archive. The idea is that it's a build system agnostic way of bundling native glue code with archives that build systems can extract the code via a command line tool, along with some metadata to describe how it should be compiled - though tbh it feels over complicated and not very practical here.

It's fairly easy to extract the C/C++ files and just integrate them in a way that suits Rust / Cargo better.

`.aar` files are simply zip archives that can be unpacked and the files under `prefab/modules/game-activity/include` can be moved to `csrc/` in this repo, which will then be built by `build.rs` via the `cc` crate.

The easiest way I found to get to the `GameActivity-release.aar` is to download the "express" agdk-libraries release from https://developer.android.com/games/agdk/download, and you should find `GameActivity-release.aar` at the top level of the archive after unpacking.

The git repo for the source code can be found here: https://android.googlesource.com/platform/frameworks/opt/gamesdk/ with the prefab code under `GameActivity/prefab-src/modules/game-activity/include` - though it may be best to synchronize with official releases.


## Minor modifications

There are a few C symbols that need to be exported from the cdylib that's built for GameActivity to load at runtime but Rust/Cargo doesn't support compiling C/C++ code in a way that can export these symbols directly and we instead have to export wrappers from Rust code.

At the bottom of GameActivity.cpp then `Java_com_google_androidgamesdk_GameActivity_loadNativeCode` should be given a `_C` suffix like `Java_com_google_androidgamesdk_GameActivity_loadNativeCode_C`

At the bottom of `android_native_app_glue.c` and `android_native_app_glue.h` `GameActivity_onCreate` should also be given a `_C` suffix like `GameActivity_onCreate_C`

Since we want to call the application's main function from Rust after initializing our own `AndroidApp` state, but we want to let applications use the same `android_main` symbol name then `android_main` should be renamed to `_rust_glue_entry` in `android_native_app_glue.h` and `android_native_app_glue.c`

One limitation discovered with the input API provided by GameActivity was that it doesn't capture keyboard scan codes which are required to properly implement a winit backend. These were the changes made to enable scan code capture:

```diff
commit 7f7df6a670d5d6dfaa315bc956210108caac57f3 (HEAD -> master)
Author: Robert Bragg <robert@sixbynine.org>
Date:   Sun May 8 02:41:48 2022 +0100

    GameActivity PATCH: capture KeyEvent scanCode

diff --git a/game-activity/csrc/game-activity/GameActivity.cpp b/game-activity/csrc/game-activity/GameActivity.cpp
index 2b37365..57bc4e1 100644
--- a/game-activity/csrc/game-activity/GameActivity.cpp
+++ b/game-activity/csrc/game-activity/GameActivity.cpp
@@ -1015,6 +1015,7 @@ static struct {
     jmethodID getModifiers;
     jmethodID getRepeatCount;
     jmethodID getKeyCode;
+    jmethodID getScanCode;
 } gKeyEventClassInfo;

 extern "C" void GameActivityKeyEvent_fromJava(JNIEnv *env, jobject keyEvent,
@@ -1046,6 +1047,8 @@ extern "C" void GameActivityKeyEvent_fromJava(JNIEnv *env, jobject keyEvent,
             env->GetMethodID(keyEventClass, "getRepeatCount", "()I");
         gKeyEventClassInfo.getKeyCode =
             env->GetMethodID(keyEventClass, "getKeyCode", "()I");
+        gKeyEventClassInfo.getScanCode =
+            env->GetMethodID(keyEventClass, "getScanCode", "()I");

         gKeyEventClassInfoInitialized = true;
     }
@@ -1070,7 +1073,9 @@ extern "C" void GameActivityKeyEvent_fromJava(JNIEnv *env, jobject keyEvent,
         /*repeatCount=*/
         env->CallIntMethod(keyEvent, gKeyEventClassInfo.getRepeatCount),
         /*keyCode=*/
-        env->CallIntMethod(keyEvent, gKeyEventClassInfo.getKeyCode)};
+        env->CallIntMethod(keyEvent, gKeyEventClassInfo.getKeyCode),
+        /*scanCode=*/
+        env->CallIntMethod(keyEvent, gKeyEventClassInfo.getScanCode)};
 }

 static bool onTouchEvent_native(JNIEnv *env, jobject javaGameActivity,
diff --git a/game-activity/csrc/game-activity/GameActivity.h b/game-activity/csrc/game-activity/GameActivity.h
index da102bc..7a5645c 100644
--- a/game-activity/csrc/game-activity/GameActivity.h
+++ b/game-activity/csrc/game-activity/GameActivity.h
@@ -262,6 +262,7 @@ typedef struct GameActivityKeyEvent {
     int32_t modifiers;
     int32_t repeatCount;
     int32_t keyCode;
+    int32_t scanCode;
 } GameActivityKeyEvent;

 /**
```

## Generate Rust bindings

Since we know we only care about android build targets then to simplify the build we pre-generate Rust bindings for the C/C++ headers using bindgen via `generate-bindings.sh`

Install bindgen via `cargo install bindgen`

`export ANDROID_NDK_ROOT=/path/to/ndk` so that `generate-bindings.sh` can find suitable sysroot headers.

Run `./generate-bindings.sh` from the top of the repo after putting the latest prefab source/headers under `csrc/`
