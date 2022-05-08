This is a minimal test application that just runs a mainloop based
on game_activity::poll_events() and traces the events received
without doing any rendering. It also saves and restores some
minimal application state.


```
rustup target add aarch64-linux-android

cargo install cargo-ndk

export ANDROID_NDK_HOME="path/to/ndk"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build

export ANDROID_HOME="path/to/sdk"
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkmainloop/.MainActivity
```