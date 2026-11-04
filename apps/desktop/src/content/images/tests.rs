use super::*;
use crate::conversation::live::tests::fixture::Fixture;
use core::prelude::v1::test;
use image::{ImageEncoder as _, Rgba, RgbaImage};

fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels = RgbaImage::from_pixel(width, height, Rgba([220, 30, 60, 128]));
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

mod decoding {
    use super::*;

    #[test]
    fn formats_and_resources() {
        for (scenario, check) in [
            (
                "preserves_pixel_properties",
                (|_scenario: &str| {
                    let image =
                        decode::decode(&png(256, 128), image::ImageFormat::Png, 128).unwrap();
                    assert_eq!(
                        image.size(0),
                        size(DevicePixels(128), DevicePixels(64)),
                        "preserves pixel properties"
                    );
                    assert_eq!(
                        &image.as_bytes(0).unwrap()[..4],
                        &[60, 30, 220, 128],
                        "preserves pixel properties"
                    );
                    assert_eq!(image.frame_count(), 1, "preserves pixel properties");
                }) as fn(&str),
            ),
            (
                "rejects_invalid_images",
                (|_scenario: &str| {
                    assert!(
                        decode::decode(b"not a PNG", image::ImageFormat::Png, 128).is_err(),
                        "rejects invalid images"
                    );
                    assert!(
                        decode::decode(&png(16_385, 1), image::ImageFormat::Png, 128).is_err(),
                        "rejects invalid images"
                    );
                    assert!(
                        decode::decode(&png(4097, 4097), image::ImageFormat::Png, 128).is_err(),
                        "rejects invalid images"
                    );
                }),
            ),
            (
                "rasterizes_svg_to_fit",
                (|_scenario: &str| {
                    let image = decode::svg(SVG.as_bytes(), 80).unwrap();
                    assert_eq!(
                        image.size(0),
                        size(DevicePixels(80), DevicePixels(40)),
                        "rasterizes svg to fit"
                    );
                    let view = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 30"><rect width="10" height="30"/></svg>"#;
                    let image = decode::svg(view.as_bytes(), 60).unwrap();
                    assert_eq!(
                        image.size(0),
                        size(DevicePixels(20), DevicePixels(60)),
                        "rasterizes svg to fit"
                    );
                }),
            ),
            (
                "rejects_unsafe_or_oversized_svg",
                (|_scenario: &str| {
                    assert_eq!(
                        decode::svg(&vec![b' '; sailry_content::media::MAX_SVG_BYTES + 1], 80)
                            .err(),
                        Some("chat_image_too_large"),
                        "rejects unsafe or oversized svg"
                    );
                    for source in [
                        r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><image href="photo.png"/></svg>"#,
                        r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><animate/></svg>"#,
                        r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#,
                        "not svg",
                    ] {
                        assert!(
                            decode::svg(source.as_bytes(), 80).is_err(),
                            "rejects unsafe or oversized svg: {source}"
                        );
                    }
                }),
            ),
            (
                "classifies_workspace_sources",
                (|_scenario: &str| {
                    let file = |path: &str| ImageSource::File {
                        worktree: sailry_protocol::WorktreeId::default(),
                        path: path.into(),
                        context: None,
                    };
                    assert!(
                        matches!(file("a.SVG").kind(), Some(source::Kind::Svg)),
                        "classifies workspace sources"
                    );
                    assert!(
                        matches!(file("a.png").kind(), Some(source::Kind::Raster(_))),
                        "classifies workspace sources"
                    );
                    assert!(
                        file("a.pdf").kind().is_none(),
                        "classifies workspace sources"
                    );
                    assert!(
                        displayable("a.svg") && displayable("a.png") && !displayable("a.pdf"),
                        "classifies workspace sources"
                    );
                    assert!(
                        !supports("a.svg"),
                        "classifies workspace sources: standalone SVG links keep their file card"
                    );
                }),
            ),
            (
                "applies_jpeg_orientation",
                (|_scenario: &str| {
                    let pixels = image::RgbImage::from_pixel(6, 4, image::Rgb([220, 30, 60]));
                    let mut bytes = Vec::new();
                    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
                    // Little-endian TIFF containing the rotate-90 orientation tag.
                    encoder
                        .set_exif_metadata(vec![
                            73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0,
                            0, 0, 0, 0,
                        ])
                        .unwrap();
                    encoder
                        .write_image(&pixels, 6, 4, image::ExtendedColorType::Rgb8)
                        .unwrap();
                    let image = decode::decode(&bytes, image::ImageFormat::Jpeg, 128).unwrap();
                    assert_eq!(
                        image.size(0),
                        size(DevicePixels(4), DevicePixels(6)),
                        "applies jpeg orientation"
                    );
                }),
            ),
            (
                "keeps_webp_alpha",
                (|_scenario: &str| {
                    let pixels = RgbaImage::from_pixel(6, 4, Rgba([220, 30, 60, 128]));
                    let mut bytes = Vec::new();
                    image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
                        .write_image(&pixels, 6, 4, image::ExtendedColorType::Rgba8)
                        .unwrap();
                    let image = decode::decode(&bytes, image::ImageFormat::WebP, 128).unwrap();
                    assert_eq!(
                        &image.as_bytes(0).unwrap()[..4],
                        &[60, 30, 220, 128],
                        "keeps webp alpha"
                    );
                }),
            ),
            (
                "first_gif_frame",
                (|_scenario: &str| {
                    let mut bytes = Vec::new();
                    {
                        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
                        encoder
                            .encode_frames(
                                [[220, 30, 60, 255], [30, 60, 220, 255]]
                                    .into_iter()
                                    .map(|color| {
                                        image::Frame::new(RgbaImage::from_pixel(6, 4, Rgba(color)))
                                    }),
                            )
                            .unwrap();
                    }
                    let image = decode::decode(&bytes, image::ImageFormat::Gif, 128).unwrap();
                    assert_eq!(image.frame_count(), 1, "first gif frame");
                    assert_eq!(
                        &image.as_bytes(0).unwrap()[..4],
                        &[60, 30, 220, 255],
                        "first gif frame"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#c33"/></svg>"##;
}

#[test]
fn scoped_file_downloads_follow_package_availability() {
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let bytes = png(32, 16);
        std::fs::write(fixture.directory.path().join("project/image.png"), &bytes).unwrap();
        let context = fixture.files_context();
        let source = ImageSource::File {
            worktree: fixture.binding.worktree.unwrap(),
            path: "image.png".into(),
            context: Some(context.clone()),
        };
        let load = || {
            fixture.runtime.block_on(load::load(
                fixture.binding.client.clone(),
                source.clone(),
                128,
                CancellationToken::new(),
                Arc::new(tokio::sync::Semaphore::new(2)),
            ))
        };
        assert!(load().is_ok());
        let download = |name: &str| {
            let target = fixture.directory.path().join(name);
            let (status, receiver) =
                tokio::sync::watch::channel(crate::downloads::transfer::Status::Preparing);
            fixture.runtime.block_on(crate::downloads::transfer::run(
                fixture.binding.client.clone(),
                crate::downloads::transfer::Source::ScopedFile {
                    context: context.clone(),
                    path: "image.png".into(),
                },
                target.clone(),
                CancellationToken::new(),
                status,
            ));
            let result = receiver.borrow().clone();
            (target, result)
        };
        let (target, status) = download("download.png");
        assert!(matches!(
            status,
            crate::downloads::transfer::Status::Finished(Ok(()))
        ));
        assert_eq!(std::fs::read(target).unwrap(), bytes);
        let sailry_protocol::Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: context.package.name.clone(),
        }) else {
            panic!("Files package expected");
        };
        fixture.execute(Command::SetPluginEnabled {
            name: context.package.name.clone(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        assert!(load().is_err());
        let (target, status) = download("disabled.png");
        assert!(matches!(
            status,
            crate::downloads::transfer::Status::Finished(Err(_))
        ));
        assert!(!target.exists());
        fixture.close();
    }
}
