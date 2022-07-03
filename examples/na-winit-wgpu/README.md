This is the same as agdk-winit-wgpu except it enables the "native-activity"
feature for winit, for running with `NativeActivity` instead of `GameActivity`

This is to see if it's practical to support multiple glue implementations with
a common API.

# Gradle Build
```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
```

# Cargo APK Build
```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

rustup target add aarch64-linux-android

cargo install cargo-apk
cargo apk run
```