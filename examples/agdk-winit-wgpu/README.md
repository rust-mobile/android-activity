This tests using the game_activity crate with winit and wgpu.

This is based on a re-worked winit backend here:
https://github.com/rib/winit/tree/agdk-game-activity

Although it would have been possible to handle the suspend/resume
lifecycle events with a simpler approach of destroying and
recreating all graphics state, this tries to represent how
lifecycle events could be handled in more complex applications,
such as within Bevy.

Considering that lifecycle events aren't supported consistently
on desktop platforms this test also aims to build and run
on desktop - for the sake of testing how more complex
applications (that need to be portable) can work. (enable
"desktop" feature to build binary)

```
rustup target add aarch64-linux-android

cargo install cargo-ndk

export ANDROID_NDK_HOME="path/to/ndk"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build

export ANDROID_HOME="path/to/sdk"
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkwinitwgpu/.MainActivity
```