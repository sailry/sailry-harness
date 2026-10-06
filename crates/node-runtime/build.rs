use std::{env, fs, path::Path};

fn files(root: &Path, directory: &Path, output: &mut String) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "bundled plugin cannot contain symlinks");
        if kind.is_dir() {
            files(root, &path, output);
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            let absolute = path.canonicalize().unwrap();
            output.push_str(&format!(
                "({relative:?}, include_bytes!({:?})),\n",
                absolute.to_str().unwrap()
            ));
        }
    }
}

fn main() {
    let manifest = Path::new("../../apps/host/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let content = fs::read_to_string(manifest).unwrap();
    let version = content
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("Host must declare its release version");
    assert!(
        !version.is_empty()
            && version.chars().all(
                |character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            ),
        "invalid Host release version"
    );
    println!("cargo:rustc-env=SAILRY_HOST_VERSION={version}");
    let root = Path::new("../../plugins");
    assert!(
        root.join("catalog.json").is_file(),
        "official plugins are missing; run git submodule update --init --recursive"
    );
    println!("cargo:rerun-if-changed={}", root.display());
    let mut output = String::from(
        "type File<'a> = (&'a str, &'a [u8]);\ntype Package<'a> = (&'a str, &'a [File<'a>]);\npub(super) const PACKAGES: &[Package<'_>] = &[\n",
    );
    let mut entries = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.join("plugin.json").is_file() {
            continue;
        }
        output.push_str(&format!("({:?}, &[\n", entry.file_name().to_str().unwrap()));
        files(&path, &path, &mut output);
        output.push_str("]),\n");
    }
    output.push_str("];\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("bundled_plugins.rs"),
        output,
    )
    .unwrap();
}
