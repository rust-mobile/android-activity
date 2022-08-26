fn main()
{
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "android" {
        println!("cargo:rustc-link-search={:}", std::env::var("OVR_OPENXR_LIBDIR").unwrap());
    }
}