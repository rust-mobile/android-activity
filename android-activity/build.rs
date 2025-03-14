#![allow(dead_code)]

fn build_glue_for_game_activity() {
    let activity_basepath = "android-games-sdk/game-activity/prefab-src/modules/game-activity/include";
    let textinput_basepath = "android-games-sdk/game-text-input/prefab-src/modules/game-text-input/include";

    for f in [
        "GameActivity.h",
        "GameActivity.cpp",
        "GameActivityEvents.h",
        "GameActivityEvents.cpp",
        "GameActivityLog.h",
    ] {
        println!("cargo:rerun-if-changed={activity_basepath}/game-activity/{f}");
    }
    cc::Build::new()
        .cpp(true)
        .include("android-games-sdk/include")
        .include(activity_basepath)
        .include(textinput_basepath)
        .file(format!("{activity_basepath}/game-activity/GameActivity.cpp"))
        .file(format!("{activity_basepath}/game-activity/GameActivityEvents.cpp"))
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_activity.a");

    for f in ["gamecommon.h", "gametextinput.h", "gametextinput.cpp"] {
        println!("cargo:rerun-if-changed={textinput_basepath}/game-text-input/{f}");
    }
    cc::Build::new()
        .cpp(true)
        .include("android-games-sdk/include")
        .include(textinput_basepath)
        .file(format!("{textinput_basepath}/game-text-input/gametextinput.cpp"))
        .cpp_link_stdlib("c++_static")
        .compile("libgame_text_input.a");

    for f in ["android_native_app_glue.h", "android_native_app_glue.c"] {
        println!("cargo:rerun-if-changed={activity_basepath}/game-activity/native_app_glue/{f}");
    }
    
    cc::Build::new()
        .include("android-games-sdk/include")
        .include(activity_basepath)
        .include(textinput_basepath)
        .include(format!("{activity_basepath}/game-activity/native_app_glue"))
        .file(format!("{activity_basepath}/game-activity/native_app_glue/android_native_app_glue.c"))
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libnative_app_glue.a");

    // We need to link to both c++_static and c++abi for the static C++ library.
    // Ideally we'd link directly to libc++.a.
    println!("cargo:rustc-link-lib=c++abi");
}

fn main() {
    #[cfg(feature = "game-activity")]
    build_glue_for_game_activity();
}
