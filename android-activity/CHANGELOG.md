# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4] - 2022-11-10
### Changed
- *Breaking*: `input_events` callback now return whether an event was handled or not to allow for fallback handling ([#31](https://github.com/rib/android-activity/issues/31))
- The native-activity backend is now implemented in Rust only, without building on `android_native_app_glue.c` ([#35](https://github.com/rib/android-activity/pull/35))
### Added
- Added `Pointer::tool_type()` API to `GameActivity` backend for compatibility with `ndk` events API ([#38](https://github.com/rib/android-activity/pull/38))

## [0.3] - 2022-09-15
### Added
- `show/hide_sot_input` API for being able to show/hide a soft keyboard (other IME still pending)
- `set_window_flags()` API for setting WindowManager params
### Changed
- *Breaking*: Created extensible, `#[non_exhaustive]` `InputEvent` wrapper enum instead of exposing `ndk` type directly

## [0.2] - 2022-08-25
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

## [0.1] - 2022-07-04
### Added
- Initial release