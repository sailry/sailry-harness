use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{Reference, desktop::Bundle},
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Disposable view resources, not another installation catalog or business store.
pub(super) struct Cache {
    directory: Option<tempfile::TempDir>,
    root: PathBuf,
    runtime: tokio::runtime::Handle,
    pub(super) images: BTreeMap<String, PathBuf>,
}

impl Cache {
    /// Run on a background worker; the caller owns the returned view cache.
    pub(super) fn create(
        bundle: Bundle,
        expected: &Reference,
        runtime: tokio::runtime::Handle,
    ) -> Result<Self, Fault> {
        if bundle.package != *expected || !bundle.valid() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid plugin view bundle",
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("sailry-plugin-")
            .tempdir()
            .map_err(io_error)?;
        let root = directory.path().canonicalize().map_err(io_error)?;
        let resources = root.join("resources");
        fs::create_dir(&resources).map_err(io_error)?;
        let mut images = BTreeMap::new();
        for (name, content) in bundle.files {
            let path = resources.join(&name);
            if path.extension().is_some_and(|extension| extension == "svg") {
                images.insert(name, path.clone());
            }
            fs::create_dir_all(path.parent().expect("resource has a parent")).map_err(io_error)?;
            create(&path, content.as_bytes())?;
        }
        for (name, encoded) in bundle.images {
            let bytes = STANDARD.decode(encoded).map_err(|_| invalid_image())?;
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(&bytes),
                image::ImageFormat::Png,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(2048);
            limits.max_image_height = Some(2048);
            limits.max_alloc = Some(16 * 1024 * 1024);
            reader.limits(limits);
            reader.decode().map_err(|_| invalid_image())?;
            let path = resources.join(&name);
            fs::create_dir_all(path.parent().expect("resource has a parent")).map_err(io_error)?;
            create(&path, &bytes)?;
            images.insert(name, path);
        }
        // The framework reads main.js. A package cannot replace the host loader
        // or use its own gpui-shell.json to select a different entry.
        let entry = serde_json::to_string(&format!("./resources/{}", bundle.entry))
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid plugin entry"))?;
        create(
            &root.join("main.js"),
            format!("export {{ default }} from {entry};\n").as_bytes(),
        )?;
        Ok(Self {
            images,
            directory: Some(directory),
            root,
            runtime,
        })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            self.runtime.spawn_blocking(move || {
                if directory.close().is_err() {
                    eprintln!("sailry-desktop: plugin view cache cleanup failed");
                }
            });
        }
    }
}

fn create(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(io_error)
}

fn io_error(_: std::io::Error) -> Fault {
    Fault::new(ErrorCode::Unavailable, "plugin view cache is unavailable")
}

fn invalid_image() -> Fault {
    Fault::new(ErrorCode::InvalidRequest, "invalid plugin PNG resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, time::Duration};

    fn bundle() -> Bundle {
        Bundle {
            package: Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            entry: "views/示例.js".into(),
            images: BTreeMap::new(),
            files: BTreeMap::from([
                (
                    "views/示例.js".into(),
                    "export default class Example {}".into(),
                ),
                ("main.js".into(), "export const value = 1;".into()),
                (
                    "images/avatar.svg".into(),
                    "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into(),
                ),
                (
                    "gpui-shell.json".into(),
                    "{\"entry\":\"outside.js\"}".into(),
                ),
            ]),
        }
    }

    #[test]
    fn resource_confinement() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let bundle = bundle();
        let expected = bundle.package.clone();
        let cache = Cache::create(bundle, &expected, runtime.handle().clone()).unwrap();
        let root = cache.root().to_path_buf();
        assert!(
            fs::read_to_string(root.join("main.js"))
                .unwrap()
                .starts_with("export { default } from \"./resources/views/")
        );
        assert_eq!(
            cache.images,
            BTreeMap::from([(
                "images/avatar.svg".into(),
                root.join("resources/images/avatar.svg")
            )])
        );
        assert!(!root.join("gpui-shell.json").exists());
        assert!(root.join("resources/gpui-shell.json").exists());
        drop(cache);
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                while root.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        });
    }

    #[test]
    fn rejects_version_and_path_conflicts() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut bundle = bundle();
        let mut expected = bundle.package.clone();
        expected.digest = "b".repeat(64);
        assert!(Cache::create(bundle.clone(), &expected, runtime.handle().clone()).is_err());
        expected = bundle.package.clone();
        bundle
            .files
            .insert("views".into(), "Not a directory".into());
        assert!(Cache::create(bundle, &expected, runtime.handle().clone()).is_err());
    }
}

#[cfg(test)]
mod png {
    use super::*;

    fn bundle(encoded: String) -> Bundle {
        Bundle {
            package: Reference {
                name: "art".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            entry: "main.js".into(),
            files: BTreeMap::from([("main.js".into(), "export default class Art {}".into())]),
            images: BTreeMap::from([("art/token.png".into(), encoded)]),
        }
    }

    #[test]
    fn preserves_transparent_pixels() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut image = image::RgbaImage::new(2, 2);
        image.put_pixel(1, 1, image::Rgba([230, 120, 40, 160]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let bundle = bundle(STANDARD.encode(bytes.get_ref()));
        let cache =
            Cache::create(bundle.clone(), &bundle.package, runtime.handle().clone()).unwrap();
        let decoded = image::open(&cache.images["art/token.png"])
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded, image);
    }

    #[test]
    fn rejects_invalid_encoding_and_non_png_content() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for encoded in ["====".into(), STANDARD.encode(b"not a PNG")] {
            let bundle = bundle(encoded);
            assert!(
                Cache::create(bundle.clone(), &bundle.package, runtime.handle().clone()).is_err()
            );
        }
    }
}
