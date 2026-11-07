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
        width: LAYOUT,
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

    #[test]
    fn size_images_in_display_pixels() {
        assert_eq!(Key::new(target(), 1, 500., 2.).pixels(), 1120);
        assert_eq!(Key::new(target(), 1, 240., 1.25).pixels(), 300);
        assert_eq!(Key::new(target(), 1, 900., 1.5).pixels(), 1440);
    }
}

mod viewport {
    use super::*;

    #[test]
    fn is_wider_than_every_card() {
        assert!(bucket(10_000.) < LAYOUT as u32);
    }

    #[test]
    fn keeps_column_cards_within_approved_height() {
        assert_eq!(MAX_HEIGHT * bucket(800.) as f32 / LAYOUT, 360.);
    }
}

mod placement {
    use super::*;

    #[test]
    fn centers_transparent_content() {
        let placement = place(&measure(8., 8., 200., 100.)).unwrap();
        assert_eq!(placement.height, 180.);
        assert_eq!(placement.x, 532.);
        assert_eq!(placement.y, 32.);
    }

    #[test]
    fn keeps_short_content_above_minimum() {
        let placement = place(&measure(0., 0., 40., 10.)).unwrap();
        assert_eq!(placement.height, MIN_HEIGHT);
        assert_eq!(placement.y, 75.);
    }

    #[test]
    fn top_aligns_tall_content() {
        let placement = place(&measure(0., 30., LAYOUT, 2000.)).unwrap();
        assert_eq!(placement.height, MAX_HEIGHT);
        assert_eq!(placement.x, 0.);
        assert_eq!(placement.y, PADDING - 30.);
    }

    #[test]
    fn keeps_desktop_pages_in_place() {
        let mut page = measure(90., 40., 1100., 600.);
        page.opaque = true;
        page.scroll_height = 900.;
        assert_eq!(
            place(&page),
            Some(Placement {
                height: MAX_HEIGHT,
                x: 0.,
                y: 0.
            })
        );
        page.height = 160.;
        page.scroll_height = 50.;
        assert_eq!(place(&page).unwrap().height, 200.);
    }

    #[test]
    fn rejects_empty_or_invalid_content() {
        assert!(place(&measure(0., 0., 0., 0.)).is_none());
        assert!(place(&measure(0., VIEWPORT, 100., 100.)).is_none());
        assert!(place(&measure(LAYOUT, 0., 100., 100.)).is_none());
        assert!(place(&measure(f32::NAN, 0., 100., 100.)).is_none());
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

    fn size(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn crops_to_placed_height_at_capture_scale() {
        let half = png(LAYOUT as u32 / 2, VIEWPORT as u32 / 2, true);
        let cropped = crop(&half, image::ImageFormat::Png, placed(200.), 640).unwrap();
        assert_eq!(size(&cropped), (640, 100));
        let short = png(LAYOUT as u32 / 2, 50, true);
        let bounded = crop(&short, image::ImageFormat::Png, placed(200.), 640).unwrap();
        assert_eq!(size(&bounded), (640, 50));
    }

    #[test]
    fn scales_layout_down_to_card_pixels() {
        let full = png(LAYOUT as u32, VIEWPORT as u32, true);
        let scaled = crop(&full, image::ImageFormat::Png, placed(MAX_HEIGHT), 800).unwrap();
        assert_eq!(size(&scaled), (800, 360));
        let half = png(LAYOUT as u32 / 2, VIEWPORT as u32 / 2, true);
        let kept = crop(&half, image::ImageFormat::Png, placed(200.), 1600).unwrap();
        assert_eq!(size(&kept), (640, 100));
    }

    #[test]
    fn rejects_blank_or_invalid_frames() {
        let format = image::ImageFormat::Png;
        assert!(crop(&png(64, 64, false), format, placed(640.), 64).is_err());
        assert!(crop(b"not an image", format, placed(640.), 64).is_err());
        assert!(crop(&png(64, 64, true), format, placed(640.), 0).is_err());
    }
}
