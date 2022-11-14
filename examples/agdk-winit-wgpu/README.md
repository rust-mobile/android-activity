This tests using `GameActivity` with winit and wgpu.

Note: This example builds against Winit master so there's always
some chance that there will be a breaking change upstream that
affects this example.

Although it would have been possible to handle the suspend/resume
lifecycle events with a simpler approach of destroying and
recreating all graphics state, this tries to represent how
lifecycle events could be handled in more complex applications,
such as within Bevy.

This example also aims to show how it's possible to use Winit
to write portable code that can run on both Android and on desktop
platforms. (enable "desktop" feature to build binary)

```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
adb shell am start -n co.realfit.agdkwinitwgpu/.MainActivity
```