
fn main() {
    cc::Build::new()
        .cpp(true)
        .include("csrc")
        .file("csrc/game-activity/GameActivity.cpp")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libgame_activity.a");
    cc::Build::new()
        .cpp(true)
        .include("csrc")
        .file("csrc/game-text-input/gametextinput.cpp")
        .cpp_link_stdlib("c++_static")
        .compile("libgame_text_input.a");
    cc::Build::new()
        .include("csrc")
        .include("csrc/game-activity/native_app_glue")
        .file("csrc/game-activity/native_app_glue/android_native_app_glue.c")
        .extra_warnings(false)
        .cpp_link_stdlib("c++_static")
        .compile("libnative_app_glue.a");
}