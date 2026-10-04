use super::*;

fn summary(revision: u64) -> plugin::Summary {
    plugin::Summary {
        name: "example".into(),
        version: Some("1".into()),
        description: Some("Example".into()),
        digest: "digest".into(),
        revision: 1,
        settings_revision: revision,
        enabled: true,
    }
}

fn settings() -> Settings {
    let live = summary(3);
    let context = Context {
        package: live.reference(),
        surface: plugin::desktop::Surface::Settings,
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
    };
    let mut settings = Settings::new(&context);
    settings.observe(&[live], true);
    settings
}

#[test]
fn adopts_only_the_matching_confirmed_receipt() {
    let mut settings = settings();
    let id = RequestId::new();
    settings.begin(id, &settings.reference()).unwrap();
    settings.observe(&[summary(4)], true);
    assert!(!settings.invalid);
    assert_eq!(settings.reference().settings_revision, 3);
    assert!(settings.finish(id, &summary(4).reference(), &summary(4)));
    assert_eq!(settings.reference().settings_revision, 4);
    assert!(settings.pending.is_none());
}

#[test]
fn invalidates_conflicting_revisions() {
    let mut settings = settings();
    let id = RequestId::new();
    settings.begin(id, &settings.reference()).unwrap();
    settings.observe(&[summary(4)], true);
    settings.failed(id);
    assert!(settings.invalid);
    assert!(settings.pending.is_none());
    assert_eq!(settings.reference().settings_revision, 3);
}

#[test]
fn same_revision_failure_preserves_the_editable_draft() {
    let mut settings = settings();
    let id = RequestId::new();
    settings.begin(id, &settings.reference()).unwrap();
    settings.failed(id);
    assert!(!settings.invalid);
    assert!(settings.pending.is_none());
}

#[test]
fn ignores_stale_metadata() {
    let mut settings = settings();
    let id = RequestId::new();
    settings.begin(id, &settings.reference()).unwrap();
    assert!(settings.finish(id, &summary(4).reference(), &summary(4)));
    settings.observe(&[summary(3)], true);
    let next = RequestId::new();
    settings.begin(next, &settings.reference()).unwrap();
    settings.failed(next);
    assert!(!settings.invalid);
    assert_eq!(settings.live.as_ref().unwrap().settings_revision, 4);
    assert_eq!(settings.reference().settings_revision, 4);
}

#[test]
fn pending_save_never_adopts_unconfirmed_metadata() {
    let mut settings = settings();
    let id = RequestId::new();
    settings.begin(id, &settings.reference()).unwrap();
    settings.observe(&[summary(4)], true);
    assert_eq!(settings.pending.as_ref().unwrap().id, id);
    assert_eq!(settings.reference().settings_revision, 3);
    settings.observe(&[summary(5)], true);
    assert!(settings.invalid);
}

#[test]
fn excludes_other_receipts_and_enablement_changes() {
    let mut settings = settings();
    settings
        .begin(RequestId::new(), &settings.reference())
        .unwrap();
    assert!(!settings.finish(RequestId::new(), &summary(4).reference(), &summary(4)));
    assert!(settings.invalid);

    let mut settings = self::settings();
    settings
        .begin(RequestId::new(), &settings.reference())
        .unwrap();
    let mut live = summary(4);
    live.enabled = false;
    settings.observe(&[live], true);
    assert!(settings.invalid);
}

#[test]
fn rejects_arbitrary_provenance_and_disconnects() {
    let mut settings = settings();
    assert!(
        settings
            .begin(RequestId::new(), &summary(4).reference())
            .is_err()
    );
    settings.observe(&[summary(3)], false);
    assert!(settings.invalid);
}
