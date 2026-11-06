use super::*;
use core::prelude::v1::test;
use sailry_protocol::{SessionId, TurnId, plugin};

fn summary(kind: Kind) -> Summary {
    Summary {
        id: ArtifactId::new(),
        kind,
        session: SessionId::new(),
        turn: TurnId::new(),
        package: plugin::Reference {
            name: "preview-fixture".into(),
            digest: "frozen".into(),
            settings_revision: 0,
        },
        title: "Chart".into(),
        summary: String::new(),
        revision: 2,
        size: 64,
        created_at: 1,
        updated_at: 1,
    }
}

static ARTIFACT: std::sync::LazyLock<ArtifactId> = std::sync::LazyLock::new(ArtifactId::new);

fn target() -> Target {
    Target {
        node: NodeId([7; 32]),
        artifact: *ARTIFACT,
        revision: 2,
    }
}

fn snapshot(height: f32) -> Snapshot {
    Snapshot {
        image: Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            Vec::new(),
        )),
        width: 640.,
        height,
    }
}

fn measure(left: f32, top: f32, width: f32, height: f32) -> Measure {
    Measure {
        left,
        top,
        width,
        height,
        scroll_height: VIEWPORT,
        opaque: false,
    }
}

mod selection {
    use super::*;

    #[test]
    fn follows_kind_and_platform() {
        assert_eq!(eligible(&summary(Kind::Preview)), SUPPORTED);
        assert!(!eligible(&summary(Kind::Document)));
        let mut invalid = summary(Kind::Preview);
        invalid.revision = 0;
        assert!(!eligible(&invalid));
    }

    #[test]
    fn targets_the_captured_revision() {
        let artifact = summary(Kind::Preview);
        let target = Target::new(NodeId([1; 32]), &artifact);
        assert_eq!(target.artifact, artifact.id);
        assert_eq!(target.revision, artifact.revision);
        assert_eq!(target.node, NodeId([1; 32]));
    }
}

mod keys {
    use super::*;

    #[test]
    fn bucket_rounds_up_within_bounds() {
        assert_eq!(bucket(500.), 560);
        assert_eq!(bucket(560.), 560);
        assert_eq!(bucket(10.), MIN_WIDTH as u32);
        assert_eq!(bucket(5000.), MAX_WIDTH as u32);
        assert_eq!(bucket(f32::NAN), MIN_WIDTH as u32);
    }

    #[test]
    fn identify_every_rendering_input() {
        let key = Key::new(target(), 1, 500., 2.);
        assert_eq!(key, Key::new(target(), 1, 520., 2.));
        assert_eq!((key.width, key.scale), (560, 200));
        assert_ne!(key, Key::new(target(), 2, 500., 2.));
        assert_ne!(key, Key::new(target(), 1, 700., 2.));
        assert_ne!(key, Key::new(target(), 1, 500., 1.));
        let mut revised = target();
        revised.revision += 1;
        assert_ne!(key, Key::new(revised, 1, 500., 2.));
        assert_eq!(Key::new(target(), 1, 500., f32::INFINITY).scale, 100);
        assert_eq!(Key::new(target(), 1, 500., 9.).scale, 400);
    }
}

mod placement {
    use super::*;

    #[test]
    fn centers_transparent_content() {
        let placement = place(&measure(8., 8., 200., 100.), 640.).unwrap();
        assert_eq!(placement.height, 148.);
        assert_eq!(placement.x, 212.);
        assert_eq!(placement.y, 16.);
    }

    #[test]
    fn keeps_short_content_above_minimum() {
        let placement = place(&measure(0., 0., 40., 10.), 640.).unwrap();
        assert_eq!(placement.height, MIN_HEIGHT);
        assert_eq!(placement.y, 43.);
    }

    #[test]
    fn top_aligns_tall_content() {
        let placement = place(&measure(0., 30., 640., 2000.), 640.).unwrap();
        assert_eq!(placement.height, MAX_HEIGHT);
        assert_eq!(placement.x, 0.);
        assert_eq!(placement.y, PADDING - 30.);
    }

    #[test]
    fn keeps_opaque_pages_in_place() {
        let mut page = measure(8., 8., 300., 100.);
        page.opaque = true;
        page.scroll_height = 900.;
        assert_eq!(
            place(&page, 640.),
            Some(Placement {
                height: MAX_HEIGHT,
                x: 0.,
                y: 0.
            })
        );
        page.scroll_height = 50.;
        assert_eq!(place(&page, 640.).unwrap().height, 108.);
    }

    #[test]
    fn rejects_empty_or_invalid_content() {
        assert!(place(&measure(0., 0., 0., 0.), 640.).is_none());
        assert!(place(&measure(0., VIEWPORT, 100., 100.), 640.).is_none());
        assert!(place(&measure(700., 0., 100., 100.), 640.).is_none());
        assert!(place(&measure(f32::NAN, 0., 100., 100.), 640.).is_none());
        assert!(place(&measure(0., 0., 100., 100.), 0.).is_none());
    }
}

mod cache {
    use super::*;

    #[test]
    fn prefers_current_theme_and_latest_width() {
        let mut cache = Cache::default();
        let old = Key::new(target(), 1, 640., 2.);
        let narrow = Key::new(target(), 2, 320., 2.);
        let wide = Key::new(target(), 2, 640., 2.);
        assert!(cache.wanted(&old));
        cache.begin(old);
        assert!(!cache.wanted(&old));
        assert!(cache.show(&target(), 2).is_none());
        cache.finish(old, Ok(snapshot(100.)));
        assert_eq!(cache.show(&target(), 2).unwrap().height, 100.);
        cache.begin(narrow);
        cache.finish(narrow, Ok(snapshot(200.)));
        cache.begin(wide);
        assert_eq!(cache.show(&target(), 2).unwrap().height, 200.);
        cache.finish(wide, Ok(snapshot(300.)));
        assert_eq!(cache.show(&target(), 2).unwrap().height, 300.);
        assert_eq!(cache.show(&target(), 1).unwrap().height, 100.);
    }

    #[test]
    fn current_failure_keeps_plain_card() {
        let mut cache = Cache::default();
        let ready = Key::new(target(), 1, 640., 2.);
        let failed = Key::new(target(), 2, 640., 2.);
        cache.begin(ready);
        cache.finish(ready, Ok(snapshot(100.)));
        cache.begin(failed);
        cache.finish(failed, Err("timed out".into()));
        assert!(!cache.wanted(&failed));
        assert!(cache.show(&target(), 2).is_none());
        assert!(cache.show(&target(), 1).is_some());
        let mut other = target();
        other.revision += 1;
        assert!(cache.show(&other, 1).is_none());
    }

    #[test]
    fn evicts_oldest_settled_entries() {
        let mut cache = Cache::default();
        let pending = Key::new(target(), 0, 640., 2.);
        cache.begin(pending);
        let mut keys = Vec::new();
        for theme in 1..=CAPACITY as u64 + 2 {
            let key = Key::new(target(), theme, 640., 2.);
            cache.begin(key);
            cache.finish(key, Ok(snapshot(theme as f32)));
            keys.push(key);
        }
        assert!(cache.wanted(&keys[0]) && cache.wanted(&keys[1]));
        assert!(!cache.wanted(&keys[2]));
        assert!(!cache.wanted(&pending));
        assert_eq!(cache.entries.len(), CAPACITY + 1);
    }
}

mod capture {
    use super::*;

    fn png(width: u32, height: u32, painted: bool) -> Vec<u8> {
        let mut pixels = image::RgbaImage::from_pixel(width, height, image::Rgba([9, 9, 9, 255]));
        if painted {
            pixels.put_pixel(1, 1, image::Rgba([200, 10, 10, 255]));
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn placed(height: f32) -> Placement {
        Placement {
            height,
            x: 0.,
            y: 0.,
        }
    }

    #[test]
    fn crops_to_placed_height_at_capture_scale() {
        let bytes = crop(
            &png(640, 816, true),
            image::ImageFormat::Png,
            320.,
            placed(100.),
        )
        .unwrap();
        let cropped = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (640, 200));
        let bounded = crop(
            &png(320, 50, true),
            image::ImageFormat::Png,
            320.,
            placed(100.),
        )
        .unwrap();
        let bounded =
            image::load_from_memory_with_format(&bounded, image::ImageFormat::Png).unwrap();
        assert_eq!(bounded.height(), 50);
    }

    #[test]
    fn rejects_blank_or_invalid_frames() {
        assert!(
            crop(
                &png(64, 64, false),
                image::ImageFormat::Png,
                64.,
                placed(32.)
            )
            .is_err()
        );
        assert!(crop(b"not an image", image::ImageFormat::Png, 64., placed(32.)).is_err());
        assert!(crop(&png(64, 64, true), image::ImageFormat::Png, 0., placed(32.)).is_err());
    }
}
