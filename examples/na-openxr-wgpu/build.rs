fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "android" {
        let android_abi = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
            "aarch64" => "arm64-v8a",
            "arm" => "armeabi-v7a",
            "x86" => "x86",
            "x86_64" => "x86_64",
            arch => {
                panic!("Unsupported architecture for Android {arch}");
            }
        };

        let libdir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join(format!("app/src/main/jniLibs/{android_abi}/lib"));
        println!("cargo:rustc-link-search={}", libdir.to_string_lossy());
    }
}
