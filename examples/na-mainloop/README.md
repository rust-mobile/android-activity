This is a minimal test application similar to `agdk-mainloop` that
is based on `NativeActivity` instead of `GameActivity`.

It shows how to run a mainloop based on `native_activity::poll_events()` and
traces the events received without doing any rendering. It also saves and
restores some minimal application state.

Since this test doesn't require a custom `Activity` subclass it's
optionally possible to build this example with `cargo apk`.

The idea here is to test whether we can have a `NativeActivity` based
glue layer which is API compatible with `game-activity`, which can
optionally be used for _very_ basic tests/demos that want the convenience
of not needing to compile any Java code.

# Gradle Build
```
rustup target add aarch64-linux-android

cargo install cargo-ndk

export ANDROID_NDK_HOME="path/to/ndk"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build

export ANDROID_HOME="path/to/sdk"
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.namainloop/.MainActivity
```

# Cargo APK Build
```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

cargo install cargo-apk
cargo apk build
cargo apk run
```