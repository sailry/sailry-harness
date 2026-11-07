use super::*;
use sailry_link::CancellationToken;

fn control() -> Control {
    Control::new(CancellationToken::new())
}

fn pattern(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
    }))
}

fn encoded(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}

fn decoded(bytes: &[u8]) -> DynamicImage {
    image::load_from_memory(bytes).unwrap()
}

#[test]
fn recognizes_image_extensions() {
    for path in ["a.png", "dir/B.JPG", "c.jpeg", "d.gif", "e.WebP", "f.bmp"] {
        assert!(named(path), "{path}");
    }
    for path in ["a.svg", "notes.txt", "png", "archive.png.zip", ""] {
        assert!(!named(path), "{path}");
    }
}

#[test]
fn passes_accepted_originals_through() {
    for (format, width) in [
        (ImageFormat::Png, 40),
        (ImageFormat::Jpeg, 40),
        (ImageFormat::Gif, 40),
        (ImageFormat::WebP, 40),
        (ImageFormat::Png, MAX_IMAGE_SIDE),
    ] {
        let original = encoded(&pattern(width, 20), format);
        let prepared = prepare(original.clone(), &control()).unwrap();
        assert!(prepared.sent.is_none(), "{format:?}");
        assert_eq!(
            prepared.source,
            ImageFacts {
                mime_type: format.to_mime_type().into(),
                width,
                height: 20,
                size: original.len() as u64,
            },
            "{format:?}"
        );
        assert_eq!(prepared.bytes, original, "{format:?}");
    }
}

#[test]
fn converts_bitmaps_and_animations() {
    let prepared = prepare(encoded(&pattern(30, 10), ImageFormat::Bmp), &control()).unwrap();
    assert_eq!(prepared.source.mime_type, "image/bmp");
    let sent = prepared.sent.unwrap();
    assert_eq!(
        (sent.mime_type.as_str(), sent.width, sent.height, sent.size),
        ("image/png", 30, 10, prepared.bytes.len() as u64)
    );
    assert_eq!(
        decoded(&prepared.bytes).to_rgb8(),
        pattern(30, 10).to_rgb8()
    );
    let mut animation = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut animation);
        let frame = |color| image::Frame::new(image::RgbaImage::from_pixel(8, 6, color));
        encoder
            .encode_frames([
                frame(image::Rgba([255, 0, 0, 255])),
                frame(image::Rgba([0, 0, 255, 255])),
            ])
            .unwrap();
    }
    let prepared = prepare(animation, &control()).unwrap();
    assert_eq!(prepared.source.mime_type, "image/gif");
    assert_eq!(prepared.sent.unwrap().mime_type, "image/png");
    let [red, _, blue, _] = decoded(&prepared.bytes).to_rgba8().get_pixel(0, 0).0;
    assert!(red > 200 && blue < 50, "the first frame is sent");
}

#[test]
fn downscales_long_sides() {
    for (format, mime_type) in [
        (ImageFormat::Png, "image/png"),
        (ImageFormat::Jpeg, "image/jpeg"),
    ] {
        let prepared = prepare(encoded(&pattern(2000, 100), format), &control()).unwrap();
        assert_eq!((prepared.source.width, prepared.source.height), (2000, 100));
        let sent = prepared.sent.unwrap();
        assert_eq!(
            (sent.mime_type.as_str(), sent.width, sent.height),
            (mime_type, MAX_IMAGE_SIDE, 78)
        );
        assert_eq!(sent.size, prepared.bytes.len() as u64);
        let image = decoded(&prepared.bytes);
        assert_eq!((image.width(), image.height()), (sent.width, sent.height));
    }
    let tall = prepare(encoded(&pattern(50, 4000), ImageFormat::Png), &control()).unwrap();
    let sent = tall.sent.unwrap();
    assert_eq!((sent.width, sent.height), (20, MAX_IMAGE_SIDE));
}

#[test]
fn composites_transparency_over_white() {
    let image = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(2, 1, |x, _| {
        image::Rgba(if x == 0 {
            [0, 0, 0, 0]
        } else {
            [10, 20, 30, 255]
        })
    }));
    let flat = opaque(&image);
    assert_eq!(flat.get_pixel(0, 0).0, [255, 255, 255]);
    assert_eq!(flat.get_pixel(1, 0).0, [10, 20, 30]);
}

#[test]
fn reads_files_with_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let original = encoded(&pattern(12, 8), ImageFormat::Png);
    std::fs::create_dir(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/pixel.png"), &original).unwrap();
    let image = read(root.as_path().into(), "assets/pixel.png".into(), &control()).unwrap();
    assert_eq!(image.path, "assets/pixel.png");
    assert_eq!(image.revision, blake3::hash(&original).to_hex().to_string());
    assert_eq!(
        image.inline_data,
        vec![InlineImage {
            mime_type: "image/png".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&original),
        }]
    );
    let mut broken = original.clone();
    broken.truncate(20);
    std::fs::write(root.join("broken.png"), broken).unwrap();
    std::fs::write(root.join("text.png"), "plain text").unwrap();
    std::fs::write(root.join("large.png"), vec![0; MAX_IMAGE_BYTES + 1]).unwrap();
    for (path, message) in [
        ("broken.png", "image could not be decoded"),
        (
            "text.png",
            "file is not a PNG, JPEG, GIF, WebP or BMP image",
        ),
        ("large.png", "image exceeds the 5 MiB read limit"),
    ] {
        let error = read(root.as_path().into(), path.into(), &control()).unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::InvalidRequest, message),
            "{path}"
        );
    }
    assert_eq!(
        read(root.as_path().into(), "missing.png".into(), &control())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert!(read(root.as_path().into(), "../outside.png".into(), &control()).is_err());
}
