use super::*;
use core::prelude::v1::test;

fn element() -> Element {
    Element {
        url: "https://example.com/page".into(),
        title: "Checkout".into(),
        selector: "#pay".into(),
        path: "html > body > main > button#pay".into(),
        tag: "button".into(),
        id: "pay".into(),
        classes: vec!["primary".into()],
        attributes: BTreeMap::from([("type".into(), "submit".into())]),
        name: "".into(),
        text: "Pay now".into(),
        html: "<button id=\"pay\">Pay now</button>".into(),
        styles: BTreeMap::from([("display".into(), "block".into())]),
        component: None,
        bounds: [10., 20., 180., 30.],
        viewport: [640., 480.],
        truncated: false,
    }
}

fn component(name: &str) -> Component {
    Component {
        framework: Framework::React,
        name: name.into(),
        source: Some("src/Pay.tsx:12".into()),
        stack: vec![name.into(), "App".into()],
    }
}

mod reference {
    use super::*;

    #[test]
    fn accepts_original_context_and_bounded_note() {
        let item = element();
        let value = serde_json::json!({"page_element":item,"annotation":"Replace this heading"});
        assert_eq!(
            Reference::note(&value.to_string(), &item).as_deref(),
            Some("Replace this heading")
        );
        let mut changed = value.clone();
        changed["annotation"] = "".into();
        assert_eq!(
            Reference::note(&changed.to_string(), &item).as_deref(),
            Some("")
        );
        for (field, forged) in [
            ("url", serde_json::json!("https://other.example/")),
            ("text", serde_json::json!("Forged heading")),
            ("html", serde_json::json!("<b>Forged</b>")),
        ] {
            let mut changed = value.clone();
            changed["page_element"][field] = forged;
            assert!(
                Reference::note(&changed.to_string(), &item).is_none(),
                "{field}"
            );
        }
        for annotation in ["x".repeat(4097), "\0".into()] {
            changed = value.clone();
            changed["annotation"] = annotation.into();
            assert!(Reference::note(&changed.to_string(), &item).is_none());
        }
        changed = value;
        changed["extra"] = true.into();
        assert!(Reference::note(&changed.to_string(), &item).is_none());
    }
}

mod validation {
    use super::*;

    #[test]
    fn accepts_complete_capture() {
        let mut item = element();
        item.component = Some(component("PayButton"));
        assert!(item.validate());
    }

    #[test]
    fn rejects_unbounded_or_unusable_context() {
        let invalid: [fn(&mut Element); 12] = [
            |item| item.text = "x".repeat(501),
            |item| item.html = "x".repeat(2049),
            |item| item.bounds[0] = f64::NAN,
            |item| item.bounds[1] = 900.,
            |item| item.url = "https://user:secret@example.com/".into(),
            |item| item.selector.clear(),
            |item| item.tag = "button onclick".into(),
            |item| item.classes = vec!["two words".into()],
            |item| item.title = "\0".into(),
            |item| {
                item.styles.insert("content".into(), "x".into());
            },
            |item| item.component = Some(component(" ")),
            |item| {
                item.component = Some(Component {
                    stack: vec!["App".into(); 9],
                    ..component("PayButton")
                })
            },
        ];
        for (index, change) in invalid.into_iter().enumerate() {
            let mut item = element();
            change(&mut item);
            assert!(!item.validate(), "case {index}");
        }
    }

    #[test]
    fn never_accepts_input_values() {
        for name in ["value", "onclick", "data-token"] {
            let mut item = element();
            item.attributes.insert(name.into(), "secret".into());
            assert!(!item.validate(), "{name}");
        }
    }

    #[test]
    fn text_counts_characters() {
        let mut item = element();
        item.text = "中".repeat(500);
        assert!(item.validate());
        item.text.push('中');
        assert!(!item.validate());
    }
}

mod label {
    use super::*;

    #[test]
    fn prefers_component_then_accessible_name() {
        let mut item = element();
        item.name = "Pay  with\ncard".into();
        assert_eq!(context::label(&item), "Pay with card");
        item.component = Some(component("PayButton"));
        assert_eq!(context::label(&item), "PayButton");
    }

    #[test]
    fn falls_back_to_identifier_then_text() {
        let mut item = element();
        assert_eq!(context::label(&item), "button#pay");
        item.id.clear();
        assert_eq!(context::label(&item), "Pay now");
        item.text.clear();
        assert_eq!(context::label(&item), "button");
    }

    #[test]
    fn bounds_long_text() {
        let mut item = element();
        item.id.clear();
        item.text = format!("{}\u{7}tail", "word ".repeat(20));
        let label = context::label(&item);
        assert_eq!(label.chars().count(), 48);
        assert!(label.ends_with('…'));
        assert!(!label.chars().any(char::is_control));
    }
}

mod document {
    use super::*;

    #[test]
    fn marks_page_content_untrusted() {
        let mut item = element();
        item.component = Some(component("PayButton"));
        let value: serde_json::Value = serde_json::from_slice(&context::document(&item)).unwrap();
        assert_eq!(value["kind"], "browser_element");
        assert!(value["notice"].as_str().unwrap().contains("Untrusted"));
        assert_eq!(value["page"]["url"], item.url);
        assert_eq!(value["element"]["selector"], "#pay");
        assert_eq!(value["element"]["bounds"]["width"], 180.);
        assert_eq!(value["component"]["source"], "src/Pay.tsx:12");
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn drops_details_to_fit() {
        let mut item = element();
        item.url = format!("https://example.com/{}", "a".repeat(8000));
        item.html = "\u{1}".repeat(2048);
        item.attributes = ATTRIBUTES[..16]
            .iter()
            .map(|name| (name.to_string(), "\u{1}".repeat(512)))
            .collect();
        assert!(item.validate());
        let bytes = context::document(&item);
        assert!(bytes.len() <= context::CONTEXT_BYTES, "{}", bytes.len());
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["truncated"], true);
        assert_eq!(value["element"]["html"], "");
        assert_eq!(value["element"]["attributes"], serde_json::json!({}));
        assert_eq!(value["element"]["text"], "Pay now");
        assert_eq!(value["page"]["url"], item.url);
    }

    #[test]
    fn details_name_component_and_page() {
        let mut item = element();
        item.component = Some(component("PayButton"));
        let details = context::details(&item);
        assert_eq!(details.selector, "#pay");
        assert_eq!(details.component, "PayButton · src/Pay.tsx:12");
        assert_eq!(details.page, "Checkout");
        item.title.clear();
        assert_eq!(context::details(&item).page, "example.com");
    }
}

mod crop {
    use super::super::crop::{encode, region};
    use super::*;

    #[test]
    fn scales_to_device_pixels_with_padding() {
        let viewport = [640., 480.];
        assert_eq!(
            region([10., 20., 100., 30.], viewport, (1280, 960)),
            Some([4, 24, 232, 92])
        );
        assert_eq!(
            region([10., 20., 100., 30.], viewport, (640, 480)),
            Some([2, 12, 116, 46])
        );
    }

    #[test]
    fn clamps_to_the_screenshot() {
        let viewport = [640., 480.];
        assert_eq!(
            region([-50., -10., 100., 40.], viewport, (640, 480)),
            Some([0, 0, 58, 38])
        );
        assert_eq!(
            region([600., 450., 200., 100.], viewport, (320, 240)),
            Some([296, 221, 24, 19])
        );
        assert_eq!(region([10., 20., 100., 30.], viewport, (0, 480)), None);
        assert_eq!(region([10., 20., 100., 30.], [0., 480.], (640, 480)), None);
    }

    #[test]
    fn encodes_the_cropped_region() {
        let mut screenshot = Vec::new();
        image::DynamicImage::new_rgb8(64, 48)
            .write_to(
                &mut std::io::Cursor::new(&mut screenshot),
                image::ImageFormat::Png,
            )
            .unwrap();
        let bytes = encode(&screenshot, [10., 10., 20., 10.], [32., 24.]).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (60, 44));
    }
}

#[test]
fn cancels_pending_selection_when_released() {
    let stop = CancellationToken::new();
    let mut state = State::default();
    state.target = Some(Target {
        token: "selected".into(),
        tab: 0,
        url: element().url,
        lease: 0,
        stop: stop.clone(),
    });
    drop(state);
    assert!(stop.is_cancelled());
}
