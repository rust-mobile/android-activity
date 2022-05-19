This crate provides a "glue" layer for building native Rust applications on Android which aims to be API compatible with the `game-activity` crate as far as possible but is based on the `NativeActivity` class instead on `GameActivity`.

The idea is to figure out if there's some minimal subset API that could potentially be factored out into a standard "glue" crate which could potentially be used by Winit. This way it might be more practical to support a wider variety of `Activity` base classes for Android applications when they don't affect how Winit works.


# Why not pure Rust?

The `android_native_app_glue` library that's provided by the NDK happens to be written in C and it has been well tested and works. Considering the somewhat fiddly thread synchronization that needs to be done for specific events like window terminations or state saves then it seems wise to re-use this logic instead of re-implementing it.

The android_native_app_glue code isn't _that_ complex and could potentially be re-implemented in Rust at some point, although it's not clear how beneficial it would be. I would note that `ndk-glue` did go down the route of re-implementing the glue layer for `NativeActivity` purely in Rust but I wasn't confident that it's synchronization model was correct. (See here for more discussion: https://github.com/rust-windowing/winit/issues/2293)


# Synchronizing with Upstream...

Upstream distribute `android_native_app_glue.c` as part of the NDK under `$ANDROID_NDK_HOME/sources/android/native_app_glue/android_native_app_glue.c`

This code is something like >10 years old and isn't expected to change.


## Minor modifications

`NativeActivity_onCreate` should be renamed to `NativeActivity_onCreate_C` because Rust/Cargo doesn't support compiling C/C++ code in a way that can export these symbols directly and we instead have to export wrappers from Rust code.

Since we want to call the application's main function from Rust after initializing our own `AndroidApp` state, but we want to let applications use the same `android_main` symbol name then `android_main` should be renamed to `_rust_glue_entry` in `android_native_app_glue.h` and `android_native_app_glue.c`

The `ID_INPUT` looper event source is disabled because we decouple polling + emitting events from input handling (I.e we expect applications to _pull_ input events when they want them instead of _push_ input events immediately). For now it's assumed that applications will explicitly check for input based as part of processing a new frame.

_(The technical difficulty with the input source is that once it triggers an event then it won't stop triggering events until all the outstanding events are read - which isn't compatible with allowing applications to explicitly check for input instead of immediately pushing input events at them (all other mainloop events will become drowned out by the input source). Unfortunately the looper API doesn't expose `epoll`'s edge triggering which would probably be ideal in this case.)_


## Generate Rust bindings

Since we know we only care about android build targets then to simplify the build we pre-generate Rust bindings for the C/C++ headers using bindgen via `generate-bindings.sh`

Install bindgen via `cargo install bindgen`

`export ANDROID_NDK_ROOT=/path/to/ndk` so that `generate-bindings.sh` can find suitable sysroot headers.

Run `./generate-bindings.sh` from the top of the repo after putting the latest prefab source/headers under `csrc/`
