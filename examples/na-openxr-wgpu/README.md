This is an example OpenXR application that builds for desktop or
Android and simply renders a head-locked gradient using Wgpu
(via Vulkan).

This borrows heavily from [here](https://github.com/Ralith/openxrs/blob/master/openxr/examples/vulkan.rs) and
[here](https://github.com/zarik5/openxrs/blob/wgpu-test/openxr/examples/vulkan.rs),
although updated to run with Wgpu 0.13 and with most of the
boilerplate for initializing OpenXR and Vulkan + Wgpu factored
into a re-usable `XrShell` that could potentially be a helpful
starting point for building more complex applications.

# Oculus Quest

To build for the Oculus Quest then you first need to download
the Oculus OpenXR Mobile SDK from:
https://developer.oculus.com/downloads/package/oculus-openxr-mobile-sdk/

and after unpacking the zip file you need to copy a suitable `libopenxr_loader.so`
library to `app/src/main/jniLibs/<abi>`

For example if building for arm64-v8a:
`cp path/to/ovr_openxr_mobile_sdk_42.0/OpenXR/Libs/Android/arm64-v8a/Debug/libopenxr_loader.so app/src/main/jniLibs/arm64-v8a`

```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/ build
./gradlew build
./gradlew installDebug
```

# Oculus Quest: Vulkan Validation Layer

To enable the Vulkan validation layer on the Oculus Quest run:
```
adb shell setprop debug.oculus.loadandinjectpackagedvvl.co.realfit.naopenxrwgpu 1
```

# Desktop

To build for PC you need to build with the "desktop" feature

`cargo run --features=desktop`