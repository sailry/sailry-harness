use super::*;
use core::prelude::v1::test;

#[test]
fn skips_single_method() {
    let mut flow = Flow::new(false, Origin::Providers);
    flow.select("single", 1);
    assert!(flow.step == Step::Configure);
    assert_eq!(flow.back(1), Some(Origin::Providers));
    assert!(flow.step == Step::Brand);
}

#[test]
fn returns_through_method_selection() {
    let mut flow = Flow::new(false, Origin::Providers);
    flow.select("multiple", 3);
    assert!(flow.step == Step::Method);
    flow.step = Step::Configure;
    assert_eq!(flow.back(3), None);
    assert!(flow.step == Step::Method);
    assert_eq!(flow.back(3), Some(Origin::Providers));
    assert!(flow.step == Step::Brand);
}

#[test]
fn editing_cannot_change_type() {
    let mut flow = Flow::new(true, Origin::Providers);
    flow.select("other", 3);
    assert_eq!(flow.back(3), None);
    assert!(flow.step == Step::Configure);
    assert!(flow.brand.is_none());
}

#[test]
fn waits_for_package_brand() {
    let mut flow = Flow::new(false, Origin::Providers);
    flow.defer("package:sample:models".into());
    assert_eq!(flow.requested.as_deref(), Some("package:sample:models"));
    assert!(flow.step == Step::Brand);
    flow.select("package:sample:models", 2);
    assert!(flow.requested.is_none());
    assert_eq!(flow.brand.as_deref(), Some("package:sample:models"));
    assert!(flow.step == Step::Method);
    flow.defer("package:other:models".into());
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

#[test]
fn returns_to_original_entry() {
    for origin in [Origin::Providers, Origin::Welcome] {
        let mut flow = Flow::new(false, origin);
        flow.select("multiple", 2);
        flow.step = Step::Configure;
        assert_eq!(flow.back(2), None);
        assert!(flow.step == Step::Method);
        assert_eq!(flow.back(2), Some(origin));
        flow.select("single", 1);
        assert_eq!(flow.back(1), Some(origin));
    }
    assert_eq!(Flow::new(true, Origin::Welcome).back(2), None);
}
