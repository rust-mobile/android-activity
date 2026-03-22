<!-- markdownlint-disable MD022 MD024 MD032 MD033  -->

# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- input: `TextInputAction` enum representing action button types on soft keyboards. ([#216](https://github.com/rust-mobile/android-activity/pull/216))
- input: `InputEvent::TextAction` event for handling action button presses from soft keyboards. ([#216](https://github.com/rust-mobile/android-activity/pull/216))
- The `ndk` and `ndk-sys` crates are now re-exported under `android_activity::ndk` and `android_activity::ndk_sys` ([#194](https://github.com/rust-mobile/android-activity/pull/194))
- `AndroidApp::java_main_looper()` gives access to the `ALooper` for the Java main / UI thread ([#198](https://github.com/rust-mobile/android-activity/pull/198))
- `AndroidApp::run_on_java_main_thread()` can be used to run boxed closures on the Java main / UI thread ([#232](https://github.com/rust-mobile/android-activity/pull/232))
- Support for an optional `android_on_create` entry point that gets called from the `Activity.onCreate()` callback before `android_main()` is called, allowing for doing some setup work on the Java main / UI thread before the `android_main` Rust code starts running.

For example:

```rust
use std::sync::OnceLock;
use android_activity::OnCreateState;
use jni::{JavaVM, refs::Global, objects::JObject};

#[unsafe(no_mangle)]
fn android_on_create(state: &OnCreateState) {
    static APP_ONCE: OnceLock<()> = OnceLock::new();
    APP_ONCE.get_or_init(|| {
        // Initialize logging...
        //
        // Remember, `android_on_create` may be called multiple times but some
        // logger crates will panic if initialized multiple times.
    });
    let vm = unsafe { JavaVM::from_raw(state.vm_as_ptr().cast()) };
    let activity = state.activity_as_ptr() as jni::sys::jobject;
    // Although the thread is implicitly already attached (we are inside an onCreate native method)
    // using `vm.attach_current_thread` here will use the existing attachment, give us an `&Env`
    // reference and also catch Java exceptions.
    if let Err(err) = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY:
        // - The `Activity` reference / pointer is at least valid until we return
        // - By creating a `Cast` we ensure we can't accidentally delete the reference
        let activity = unsafe { env.as_cast_raw::<JObject>(&activity)? };

        // Do something with the activity on the Java main thread...
        Ok(())
    }) {
       eprintln!("Failed to interact with Android SDK on Java main thread: {err:?}");
    }
}
```

- Support for `MotionEvent` history, providing higher fidelity input data for things like stylus input (`native-activity` + `game-activity` backends). ([#218](https://github.com/rust-mobile/android-activity/pull/218))


### Changed

- rust-version bumped to 1.85.0 ([#193](https://github.com/rust-mobile/android-activity/pull/193), [#219](https://github.com/rust-mobile/android-activity/pull/219))
- GameActivity updated to 4.4.0 ([#191](https://github.com/rust-mobile/android-activity/pull/191), [#240](https://github.com/rust-mobile/android-activity/pull/240))

**Important:** This release is no longer compatible with GameActivity 2.0.2

**Android Packaging:** Your Android application must be packaged with the corresponding androidX, GameActivity 4.x.x library from Google.

This release has been tested with the [`androidx.games:games-activity:4.4.0` stable
release](https://developer.android.com/jetpack/androidx/releases/games#games-activity-4.4.0), and is backwards
compatible with the 4.0.0 stable release.

If you use Gradle to build your Android application, you can depend on the 4.4.0 release of the GameActivity library via:

```gradle
dependencies {
    implementation 'androidx.appcompat:appcompat:1.7.1'

    // To use the Games Activity library
    implementation "androidx.games:games-activity:4.4.0"
    // Note: don't include game-text-input separately, since it's integrated into game-activity
}
```

Note: there is no guarantee that later 4.x.x releases of GameActivity will be compatible with this release of
`android-activity`, so please refer to the `android-activity` release notes for any future updates regarding
GameActivity compatibility.

### Fixed

- *Safety* `AndroidApp::asset_manager()` returns an `AssetManager` that has a safe `'static` lifetime that's not invalidated when `android_main()` returns ([#233](https://github.com/rust-mobile/android-activity/pull/233))
- *Safety* The `native-activity` backend clears its `ANativeActivity` ptr after `onDestroy` and `AndroidApp` remains safe to access after `android_main()` returns ([#234](https://github.com/rust-mobile/android-activity/pull/234))
- *Safety* `AndroidApp::activity_as_ptr()` returns a pointer to a global reference that remains valid until `AndroidApp` is dropped, instead of the `ANativeActivity`'s `clazz` pointer which is only guaranteed to be valid until `onDestroy` returns (`native-activity` backend) ([#234](https://github.com/rust-mobile/android-activity/pull/234))
- *Safety* The `game-activity` backend clears its `android_app` ptr after `onDestroy` and `AndroidApp` remains safe to access after `android_main()` returns ([#236](https://github.com/rust-mobile/android-activity/pull/236))
- Support for `AndroidApp::show/hide_soft_input()` APIs in the `native-activity` backend ([#178](https://github.com/rust-mobile/android-activity/pull/178))

## [0.6.0] - 2024-04-26

### Changed
- rust-version bumped to 1.69.0 ([#156](https://github.com/rust-mobile/android-activity/pull/156))
- Upgrade to `ndk-sys 0.6.0` and `ndk 0.9.0` ([#155](https://github.com/rust-mobile/android-activity/pull/155))

### Fixed
- Check for null `saved_state_in` pointer from `NativeActivity`

## [0.5.2] - 2024-01-30

### Fixed
- NativeActivity: OR with `EVENT_ACTION_MASK` when extracting action from `MotionEvent` - fixing multi-touch input ([#146](https://github.com/rust-mobile/android-activity/issues/146), [#147](https://github.com/rust-mobile/android-activity/pull/147))

## [0.5.1] - 2023-12-20

### Changed
- Avoids depending on default features for `ndk` crate to avoid pulling in any `raw-window-handle` dependencies ([#142](https://github.com/rust-mobile/android-activity/pull/142))

  **Note:** Technically, this could be observed as a breaking change in case you
  were depending on the `rwh_06` feature that was enabled by default in the
  `ndk` crate. This could be observed via the `NativeWindow` type (exposed via
  `AndroidApp::native_window()`) no longer implementing `rwh_06::HasWindowHandle`.

  In the unlikely case that you were depending on the `ndk`'s `rwh_06` API
  being enabled by default via `android-activity`'s `ndk` dependency, your crate
  should explicitly enable the `rwh_06` feature for the `ndk` crate.

  As far as could be seen though, it's not expected that anything was
  depending on this (e.g. anything based on Winit enables the `ndk` feature
  based on an equivalent `winit` feature).

  The benefit of the change is that it can help avoid a redundant
  `raw-window-handle 0.6` dependency in projects that still need to use older
  (non-default) `raw-window-handle` versions. (Though note that this may be
  awkward to achieve in practice since other crates that depend on the `ndk`
  are still likely to use default features and also pull in
  `raw-window-handles 0.6`)

- The IO thread now gets named `stdio-to-logcat` and main thread is named `android_main` ([#145](https://github.com/rust-mobile/android-activity/pull/145))
- Improved IO error handling in `stdio-to-logcat` IO loop. ([#133](https://github.com/rust-mobile/android-activity/pull/133))

## [0.5.0] - 2023-10-16
### Added
- Added `MotionEvent::action_button()` exposing the button associated with button press/release actions ([#138](https://github.com/rust-mobile/android-activity/pull/138))

### Changed
- rust-version bumped to 0.68 ([#123](https://github.com/rust-mobile/android-activity/pull/123))
- *Breaking*: updates to `ndk 0.8` and `ndk-sys 0.5` ([#128](https://github.com/rust-mobile/android-activity/pull/128))
- The `Pointer` and `PointerIter` types from the `ndk` crate are no longer directly exposed in the public API ([#122](https://github.com/rust-mobile/android-activity/pull/122))
- All input API enums based on Android SDK enums have been made runtime extensible via hidden `__Unknown(u32)` variants ([#131](https://github.com/rust-mobile/android-activity/pull/131))

## [0.5.0-beta.1] - 2023-08-15
### Changed
- Pulled in `ndk-sys 0.5.0-beta.0` and `ndk 0.8.0-beta.0` ([#113](https://github.com/rust-mobile/android-activity/pull/113))

## [0.5.0-beta.0] - 2023-08-15

### Added
- Added `KeyEvent::meta_state()` for being able to query the state of meta keys, needed for character mapping ([#102](https://github.com/rust-mobile/android-activity/pull/102))
- Added `KeyCharacterMap` JNI bindings to the corresponding Android SDK API ([#102](https://github.com/rust-mobile/android-activity/pull/102))
- Added `AndroidApp::device_key_character_map()` for being able to get a `KeyCharacterMap` for a given `device_id` for unicode character mapping ([#102](https://github.com/rust-mobile/android-activity/pull/102))

    <details>
    <summary>Click here for an example of how to handle unicode character mapping:</summary>

    ```rust
    let mut combining_accent = None;
    // Snip


    let combined_key_char = if let Ok(map) = app.device_key_character_map(device_id) {
        match map.get(key_event.key_code(), key_event.meta_state()) {
            Ok(KeyMapChar::Unicode(unicode)) => {
                let combined_unicode = if let Some(accent) = combining_accent {
                    match map.get_dead_char(accent, unicode) {
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
                combining_accent = None;
                combined_unicode.map(|unicode| KeyMapChar::Unicode(unicode))
            }
            Ok(KeyMapChar::CombiningAccent(accent)) => {
                info!("KeyEvent: Pressed 'dead key' combining accent '{accent}'");
                combining_accent = Some(accent);
                Some(KeyMapChar::CombiningAccent(accent))
            }
            Ok(KeyMapChar::None) => {
                info!("KeyEvent: Pressed non-unicode key");
                combining_accent = None;
                None
            }
            Err(err) => {
                log::error!("KeyEvent: Failed to get key map character: {err:?}");
                combining_accent = None;
                None
            }
        }
    } else {
        None
    };
    ```

    </details>
- Added `TextEvent` Input Method event for supporting text editing via virtual keyboards ([#24](https://github.com/rust-mobile/android-activity/pull/24))

### Changed
- GameActivity updated to 2.0.2 (requires the corresponding 2.0.2 `.aar` release from Google) ([#88](https://github.com/rust-mobile/android-activity/pull/88))
- `AndroidApp::input_events()` is replaced by `AndroidApp::input_events_iter()` ([#102](https://github.com/rust-mobile/android-activity/pull/102))

    <details>
    <summary>Click here for an example of how to use `input_events_iter()`:</summary>

    ```rust
    match app.input_events_iter() {
        Ok(mut iter) => {
            loop {
                let read_input = iter.next(|event| {
                    let handled = match event {
                        InputEvent::KeyEvent(key_event) => {
                            // Snip
                        }
                        InputEvent::MotionEvent(motion_event) => {
                            // Snip
                        }
                        event => {
                            // Snip
                        }
                    };

                    handled
                });

                if !read_input {
                    break;
                }
            }
        }
        Err(err) => {
            log::error!("Failed to get input events iterator: {err:?}");
        }
    }
    ```

    </details>

## [0.4.3] - 2023-07-30
### Fixed
- Fixed a deadlock in the `native-activity` backend while waiting for the native thread after getting an `onDestroy` callback from Java ([#94](https://github.com/rust-mobile/android-activity/pull/94))
- Fixed numerous deadlocks in the `game-activity` backend with how it would wait for the native thread in various Java callbacks, after the app has returned from `android_main` ([#98](https://github.com/rust-mobile/android-activity/pull/98))

## [0.4.2] - 2023-06-17
### Changed
- The `Activity.finish()` method is now called when `android_main` returns so the `Activity` will be destroyed ([#67](https://github.com/rust-mobile/android-activity/issues/67))
- The `native-activity` backend now propagates `NativeWindow` redraw/resize and `ContentRectChanged` callbacks to main loop ([#70](https://github.com/rust-mobile/android-activity/pull/70))
- The `game-activity` implementation of `pointer_index()` was fixed to not always return `0` ([#80](https://github.com/rust-mobile/android-activity/pull/84))
- Added `panic` guards around application's `android_main()` and native code that could potentially unwind across a Java FFI boundary ([#68](https://github.com/rust-mobile/android-activity/pull/68))

## [0.4.1] - 2023-02-16
### Added
- Added `AndroidApp::vm_as_ptr()` to expose JNI `JavaVM` pointer ([#60](https://github.com/rust-mobile/android-activity/issues/60))
- Added `AndroidApp::activity_as_ptr()` to expose Android `Activity` JNI reference as pointer ([#60](https://github.com/rust-mobile/android-activity/issues/60))
### Changed
- Removed some overly-verbose logging in the `native-activity` backend ([#49](https://github.com/rust-mobile/android-activity/pull/49))
### Removed
- Most of the examples were moved to <https://github.com/rust-mobile/rust-android-examples> ([#50](https://github.com/rust-mobile/android-activity/pull/50))

## [0.4.0] - 2022-11-10
### Changed
- *Breaking*: `input_events` callback now return whether an event was handled or not to allow for fallback handling ([#31](https://github.com/rust-mobile/android-activity/issues/31))
- The native-activity backend is now implemented in Rust only, without building on `android_native_app_glue.c` ([#35](https://github.com/rust-mobile/android-activity/pull/35))
### Added
- Added `Pointer::tool_type()` API to `GameActivity` backend for compatibility with `ndk` events API ([#38](https://github.com/rust-mobile/android-activity/pull/38))

## [0.3.0] - 2022-09-15
### Added
- `show/hide_sot_input` API for being able to show/hide a soft keyboard (other IME still pending)
- `set_window_flags()` API for setting WindowManager params
### Changed
- *Breaking*: Created extensible, `#[non_exhaustive]` `InputEvent` wrapper enum instead of exposing `ndk` type directly

## [0.2.0] - 2022-08-25
### Added
- Emit an `InputAvailable` event for new input with `NativeActivity` and `GameActivity`
  enabling gui apps that don't render continuously
- Oboe and Cpal audio examples added
- `AndroidApp` is now `Send` + `Sync`
### Changed
- *Breaking*: updates to `ndk 0.7` and `ndk-sys 0.4`
- *Breaking*: `AndroidApp::config()` now returns a clonable `ConfigurationRef` instead of a deep `Configuration` copy
### Removed
- The `NativeWindowRef` wrapper struct was removed since `NativeWindow` now implements `Clone` and `Drop` in `ndk 0.7`
- *Breaking*: The `FdEvent` and `Error` enum values were removed from `PollEvents`

## [0.1.1] - 2022-07-04
### Changed
- Documentation fixes

## [0.1.0] - 2022-07-04
### Added
- Initial release

[unreleased]: https://github.com/rust-mobile/android-activity/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/rust-mobile/android-activity/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/rust-mobile/android-activity/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/rust-mobile/android-activity/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/rust-mobile/android-activity/compare/v0.4.3...v0.5.0
[0.4.3]: https://github.com/rust-mobile/android-activity/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/rust-mobile/android-activity/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/rust-mobile/android-activity/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/rust-mobile/android-activity/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/rust-mobile/android-activity/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/rust-mobile/android-activity/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/rust-mobile/android-activity/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/rust-mobile/android-activity/releases/tag/v0.1.0
