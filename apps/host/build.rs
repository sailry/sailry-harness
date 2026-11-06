#[path = "../revision.rs"]
mod revision;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    revision::embed();
}
