use super::*;
use core::prelude::v1::test;

#[test]
fn navigation_and_editing() {
    for origin in [Origin::Providers, Origin::Welcome] {
        let mut flow = Flow::new(false, origin);
        flow.select("single", 1);
        assert!(flow.step == Step::Configure, "single method: configure");
        assert_eq!(flow.back(1), Some(origin), "single method: origin");
        assert!(flow.step == Step::Brand, "single method: brand");
        for methods in [2, 3] {
            flow.select("multiple", methods);
            assert!(flow.step == Step::Method, "{methods} methods: select");
            flow.step = Step::Configure;
            assert_eq!(flow.back(methods), None, "{methods} methods: back");
            assert!(flow.step == Step::Method, "{methods} methods: method step");
            assert_eq!(
                flow.back(methods),
                Some(origin),
                "{methods} methods: origin"
            );
            assert!(flow.step == Step::Brand, "{methods} methods: brand");
        }
        flow.select("single", 1);
        assert_eq!(flow.back(1), Some(origin), "restored single method");
    }
    let mut flow = Flow::new(true, Origin::Providers);
    flow.select("other", 3);
    assert_eq!(flow.back(3), None, "editing: no type navigation");
    assert!(flow.step == Step::Configure, "editing: configure");
    assert!(flow.brand.is_none(), "editing: no replacement brand");
    assert_eq!(
        Flow::new(true, Origin::Welcome).back(2),
        None,
        "editing welcome"
    );
}

#[test]
fn login_returns_to_picker() {
    // A login shares its brand with API methods but skips the method step.
    let mut flow = Flow::new(false, Origin::Providers);
    let entries = presets::bundled();
    let login = Choice::Login {
        brand: "openai".into(),
        id: "chat-gpt-oauth".into(),
    };
    let (brand, methods) = brands::resolve(&entries, &login).unwrap();
    flow.select(&brand, methods.len());
    assert!(flow.step == Step::Configure);
    let count = method_count(&entries, Some(&brand), methods.first());
    assert_eq!(count, 1);
    assert_eq!(flow.back(count), Some(Origin::Providers));
    let (brand, methods) = brands::resolve(&entries, &Choice::Api("openai".into())).unwrap();
    flow.select(&brand, methods.len());
    assert!(flow.step == Step::Method);
    flow.step = Step::Configure;
    let count = method_count(&entries, Some(&brand), methods.first());
    assert_eq!(count, 2);
    assert_eq!(flow.back(count), None);
    assert!(flow.step == Step::Method);
}

#[test]
fn partner_returns_to_picker() {
    // A suggestion replaces the brand, whose own list decides the back target.
    assert_eq!(
        method_count(&presets::bundled(), Some("partner:sample"), None),
        0
    );
}

#[test]
fn waits_for_package_brand() {
    let mut flow = Flow::new(false, Origin::Providers);
    let requested = Choice::Api("package:sample:models".into());
    flow.defer(requested.clone());
    assert_eq!(flow.requested, Some(requested));
    assert!(flow.step == Step::Brand);
    flow.select("package:sample:models", 2);
    assert!(flow.requested.is_none());
    assert_eq!(flow.brand.as_deref(), Some("package:sample:models"));
    assert!(flow.step == Step::Method);
    flow.defer(Choice::Login {
        brand: "package:other:models".into(),
        id: "sign-in".into(),
    });
    flow.select("openai", 3);
    assert!(flow.requested.is_none());
    assert_eq!(flow.brand.as_deref(), Some("openai"));
}

mod access {
    use super::*;

    #[test]
    fn classifies_methods() {
        assert_eq!(Access::of(Preset::OpenAi, false, false), Access::Key);
        assert_eq!(
            Access::of(Preset::CompatibleChat, false, true),
            Access::Endpoint
        );
        assert_eq!(Access::of(Preset::AzureOpenAi, false, true), Access::Cloud);
        assert_eq!(Access::of(Preset::ChatGptOAuth, true, false), Access::Login);
    }

    #[test]
    fn summary_never_repeats_family() {
        let family = SharedString::from("OpenAI");
        assert_eq!(
            method_summary(&family, "OpenAI".into(), Access::Key),
            tr("provider_access_key")
        );
        assert_eq!(
            method_summary(&family, "ChatGPT OAuth".into(), Access::Login),
            SharedString::from("ChatGPT OAuth")
        );
    }
}
