This is a minimal test application based on `GameActivity` that just
runs a mainloop based on android_activity::poll_events() and plays a
sine wave audio test using the Cpal audio library.

```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkcpal/.MainActivity
```
