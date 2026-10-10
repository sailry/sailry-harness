fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Dart loads the bridge independently of its executable, so native speech
    // libraries must resolve beside the bridge rather than beside the Dart SDK.
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => println!("cargo:rustc-link-arg-cdylib=-Wl,-rpath,@loader_path"),
        Ok("linux") => println!("cargo:rustc-link-arg-cdylib=-Wl,-rpath,$ORIGIN"),
        _ => {}
    }
}
