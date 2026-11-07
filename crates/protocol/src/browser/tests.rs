use super::*;
use serde_json::json;

mod screenshot {
    use super::*;

    #[test]
    fn defaults_to_viewport_jpeg() {
        let action: Action =
            serde_json::from_value(json!({"action":"screenshot","tab":3})).unwrap();
        assert_eq!(
            action,
            Action::Screenshot {
                tab: 3,
                target: None,
                scale: Scale::Css,
                format: Format::Jpeg,
                save: false,
            }
        );
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            json!({"action":"screenshot","tab":3,"scale":"css","format":"jpeg","save":false})
        );
    }

    #[test]
    fn round_trips_targets() {
        for target in [
            json!({"kind":"selector","selector":"#submit"}),
            json!({"kind":"element","snapshot":"captured","element":4}),
            json!({"kind":"rect","x":10,"y":20,"width":300,"height":40}),
        ] {
            let value = json!({
                "action":"screenshot","tab":1,"target":target,
                "scale":"device","format":"png","save":true
            });
            let action: Action = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&action).unwrap(), value);
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        for value in [
            json!({"action":"screenshot","tab":1,"path":"capture.jpg"}),
            json!({"action":"screenshot","tab":1,"target":{"kind":"selector","selector":"a","extra":1}}),
            json!({"action":"screenshot","tab":1,"target":{"kind":"page"}}),
            json!({"action":"screenshot","tab":1,"target":{"kind":"rect","x":-1,"y":0,"width":1,"height":1}}),
            json!({"action":"screenshot","tab":1,"format":"webp"}),
            json!({"action":"screenshot","tab":1,"scale":2}),
        ] {
            assert!(serde_json::from_value::<Action>(value).is_err());
        }
    }

    #[test]
    fn approves_only_saved_captures() {
        let capture = |save| Action::Screenshot {
            tab: 0,
            target: None,
            scale: Scale::default(),
            format: Format::default(),
            save,
        };
        assert!(!capture(false).requires_approval());
        assert!(capture(true).requires_approval());
        assert!(capture(false).read_only());
        assert!(capture(true).read_only());
    }

    #[test]
    fn names_encoded_formats() {
        assert_eq!(
            (Format::Jpeg.mime(), Format::Jpeg.extension()),
            ("image/jpeg", "jpg")
        );
        assert_eq!(
            (Format::Png.mime(), Format::Png.extension()),
            ("image/png", "png")
        );
    }
}
