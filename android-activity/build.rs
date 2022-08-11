#![allow(dead_code)]

fn build_glue_for_native_activity() {
    cc::Build::new()
        .include("native-activity-csrc")
        .include("native-activity-csrc/native-activity/native_app_glue")
        .file("native-activity-csrc/native-activity/native_app_glue/android_native_app_glue.c")
        .compile("libnative_app_glue.a");
}

fn build_glue_for_game_activity() {
    cc::Build::new()
        .cpp(true)
        .include("game-activity-csrc")
        .file("game-activity-csrc/game-activity/GameActivity.cpp")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_activity.a");
    cc::Build::new()
        .cpp(true)
        .include("game-activity-csrc")
        .file("game-activity-csrc/game-text-input/gametextinput.cpp")
        .cpp_link_stdlib("c++_static")
        .compile("libgame_text_input.a");
    cc::Build::new()
        .include("game-activity-csrc")
        .include("game-activity-csrc/game-activity/native_app_glue")
        .file("game-activity-csrc/game-activity/native_app_glue/android_native_app_glue.c")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libnative_app_glue.a");
}

fn main() {
    #[cfg(feature = "game-activity")]
    build_glue_for_game_activity();
    #[cfg(feature = "native-activity")]
    build_glue_for_native_activity();
}
