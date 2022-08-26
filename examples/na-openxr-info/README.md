This is a minimal OpenXR application that builds for desktop or
Android and simply prints out extension information for the OpenXR
library.

This is based on the [hello](https://github.com/Ralith/openxrs/blob/master/openxr/examples/hello.rs)
example from the [openxrs](https://github.com/Ralith/openxrs) repo.

# Oculus Quest

To build for the Oculus Quest then you first need to download
the Oculus OpenXR Mobile SDK from:
https://developer.oculus.com/downloads/package/oculus-openxr-mobile-sdk/

unpack the zip file and then set the OVR_OPENXR_LIBDIR environment variable
to point at the directory with libopenxr_loader.so

```
export export OVR_OPENXR_LIBDIR="path/to/ovr_openxr_mobile_sdk_42.0/OpenXR/Libs/Android/arm64-v8a/Debug"
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build --features=android
./gradlew build
./gradlew installDebug
```

# Desktop

To build for PC you need to build with the "desktop" feature

`cargo run --features=desktop`