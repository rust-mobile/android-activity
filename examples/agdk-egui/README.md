This tests using `GameActivity` with egui, winit and wgpu.

This is based on a re-worked winit backend here:
https://github.com/rib/winit/tree/android-activity

```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkegui/.MainActivity
```
