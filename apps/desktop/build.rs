fn main() {
    println!("cargo:rerun-if-changed=../../locales");
    println!("cargo:rerun-if-changed=windows/app.rc");
    println!("cargo:rerun-if-changed=../../assets/branding/sailry.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        // GPUI b9b7df7 loads the application's window icon from resource ID 1.
        embed_resource::compile_for(
            "windows/app.rc",
            ["sailry-desktop"],
            embed_resource::ParamsIncludeDirs(["../../assets/branding"]),
        )
        .manifest_required()
        .expect("failed to embed the Sailry application icon");
    }
}
