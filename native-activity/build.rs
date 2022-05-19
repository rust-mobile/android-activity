
fn main() {
    cc::Build::new()
        .include("csrc")
        .include("csrc/native-activity/native_app_glue")
        .file("csrc/native-activity/native_app_glue/android_native_app_glue.c")
        .compile("libnative_app_glue.a");
}