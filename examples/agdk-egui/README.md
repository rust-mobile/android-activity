This tests using the game_activity crate with egui, winit and wgpu.

This is based on a re-worked winit backend here:
https://github.com/rib/winit/tree/agdk-game-activity

and based on updated egui-winit and egui-wgpu crates here:
https://github.com/rib/egui/tree/android-deferred-winit-wgpu

```
rustup target add aarch64-linux-android

cargo install cargo-ndk

export ANDROID_NDK_HOME="path/to/ndk"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build

export ANDROID_HOME="path/to/sdk"
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkegui/.MainActivity
```