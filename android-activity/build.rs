fn build_glue_for_game_activity() {
    for f in [
        "GameActivity.h",
        "GameActivity.cpp",
        "GameActivityEvents.h",
        "GameActivityEvents.cpp",
        "GameActivityLog.h",
    ] {
        println!("cargo:rerun-if-changed=game-activity-csrc/game-activity/{f}");
    }
    cc::Build::new()
        .cpp(true)
        .include("game-activity-csrc")
        .file("game-activity-csrc/game-activity/GameActivity.cpp")
        .file("game-activity-csrc/game-activity/GameActivityEvents.cpp")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_activity.a");

    for f in ["gamecommon.h", "gametextinput.h", "gametextinput.cpp"] {
        println!("cargo:rerun-if-changed=game-activity-csrc/game-text-input/{f}");
    }
    cc::Build::new()
        .cpp(true)
        .include("game-activity-csrc")
        .file("game-activity-csrc/game-text-input/gametextinput.cpp")
        .cpp_link_stdlib("c++_static")
        .compile("libgame_text_input.a");

    for f in ["android_native_app_glue.h", "android_native_app_glue.c"] {
        println!("cargo:rerun-if-changed=game-activity-csrc/native_app_glue/{f}");
    }
    cc::Build::new()
        .include("game-activity-csrc")
        .include("game-activity-csrc/game-activity/native_app_glue")
        .file("game-activity-csrc/game-activity/native_app_glue/android_native_app_glue.c")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libnative_app_glue.a");

    // We need to link to both c++_static and c++abi for the static C++ library.
    // Ideally we'd link directly to libc++.a.
    println!("cargo:rustc-link-lib=c++abi");
}

fn main() {
    // Avoid re-running build script if nothing changed.
    println!("cargo:rerun-if-changed=build.rs");

    if cfg!(feature = "game-activity") {
        build_glue_for_game_activity();
    }

    // Whether this is used directly in or as a dependency on docs.rs.
    println!("cargo:rustc-check-cfg=cfg(used_on_docsrs)");
    if std::env::var("DOCS_RS").is_ok() {
        println!("cargo:rustc-cfg=used_on_docsrs");
    }
}
