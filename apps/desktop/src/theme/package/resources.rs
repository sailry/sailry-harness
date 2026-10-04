use super::*;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, PathBuf},
};

const MANIFEST_LIMIT: usize = 256 * 1024;
const FILE_LIMIT: usize = 8 * 1024 * 1024;
const PACKAGE_LIMIT: usize = 32 * 1024 * 1024;
const DECODE_LIMIT: u64 = 32 * 1024 * 1024;

pub(super) fn validate_path(path: &str) -> Result<(), String> {
    if !path.starts_with("assets/")
        || path.len() > 512
        || path.contains(['\\', ':', '?', '#'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("theme resource must be a normalized path under assets/".into());
    }
    match ImageFormat::from_path(path) {
        Ok(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif) => Ok(()),
        _ => Err("unsupported theme image format".into()),
    }
}

fn confined(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    if fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("theme directory cannot be a symbolic link".into());
    }
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err("invalid theme resource path".into());
        };
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("theme resources cannot use symbolic links".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(path)
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("theme resource exceeds file limit".into());
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("theme resource exceeds file limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("theme resource exceeds file limit".into());
    }
    Ok(bytes)
}

pub(super) fn load(directory: &Path, strict: bool) -> Result<Loaded, String> {
    let bytes = read(&confined(directory, "theme.json")?, MANIFEST_LIMIT)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest.validate()?;
    let mut files = BTreeMap::from([("theme.json".into(), bytes)]);
    let mut images = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    let mut encoded = files["theme.json"].len();
    let mut decoded = 0;
    for variant in [&manifest.light, &manifest.dark] {
        for asset in variant.assets.values() {
            if !seen.insert(asset.path.clone()) {
                continue;
            }
            // A path-boundary violation invalidates the package even during recovery.
            let path = confined(directory, &asset.path)?;
            let result: Result<(), String> = (|| {
                let bytes = read(&path, FILE_LIMIT)?;
                if encoded + bytes.len() > PACKAGE_LIMIT || images.len() >= 32 {
                    return Err("theme package exceeds resource limit".into());
                }
                let (image, allocation) = decode(
                    &bytes,
                    ImageFormat::from_path(&path).map_err(|e| e.to_string())?,
                    DECODE_LIMIT - decoded,
                )?;
                encoded += bytes.len();
                decoded += allocation;
                files.insert(asset.path.clone(), bytes);
                images.insert(asset.path.clone(), image);
                Ok(())
            })();
            if let Err(error) = result {
                if strict {
                    return Err(format!("{}: {error}", asset.path));
                }
                warnings.push(format!("{}: {error}", asset.path));
            }
        }
    }
    Ok(Loaded {
        manifest,
        images,
        warnings,
        files,
    })
}

fn decode(
    bytes: &[u8],
    format: ImageFormat,
    remaining: u64,
) -> Result<(Arc<RenderImage>, u64), String> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(remaining);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    let allocation = u64::from(width) * u64::from(height) * 4;
    if width == 0 || height == 0 || allocation.max(decoder.total_bytes()) > remaining {
        return Err("theme image exceeds decoded resource limit".into());
    }
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    let mut pixels = image.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok((
        Arc::new(RenderImage::new([image::Frame::new(pixels)])),
        allocation,
    ))
}

pub(super) fn install(loaded: &Loaded, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root)?;
    let destination = root.join(&loaded.manifest.id);
    if destination.try_exists()? {
        return Err("theme package is already installed".into());
    }
    let staging = tempfile::tempdir_in(root)?;
    for (relative, bytes) in &loaded.files {
        let path = staging.path().join(relative);
        fs::create_dir_all(path.parent().ok_or("theme resource directory is missing")?)?;
        fs::write(path, bytes)?;
    }
    fs::rename(staging.path(), destination)?;
    Ok(())
}
