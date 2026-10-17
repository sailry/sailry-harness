use std::{fs, path::Path};

fn main() {
    let manifest = Path::new("../../apps/host/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let source = fs::read_to_string(manifest).expect("Host manifest must exist");
    let version = source
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("Host must declare its product version");
    println!("cargo:rustc-env=SAILRY_PRODUCT_VERSION={version}");
}
