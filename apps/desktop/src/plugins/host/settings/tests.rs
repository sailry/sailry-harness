use super::*;

fn summary(revision: u64) -> plugin::Summary {
    plugin::Summary {
        compatibility: Default::default(),
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
fn revisions_and_receipts() {
    for (scenario, check) in [
        (
            "adopts_only_the_matching_confirmed_receipt",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                settings.observe(&[summary(4)], true);
                assert!(
                    !settings.invalid,
                    "adopts only the matching confirmed receipt"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    3,
                    "adopts only the matching confirmed receipt"
                );
                assert!(
                    settings.finish(id, &summary(4).reference()),
                    "adopts only the matching confirmed receipt"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    4,
                    "adopts only the matching confirmed receipt"
                );
                assert!(
                    settings.pending.is_none(),
                    "adopts only the matching confirmed receipt"
                );
            }) as fn(&str),
        ),
        (
            "adopts_observed_revisions",
            (|_scenario: &str| {
                let mut settings = settings();
                let previous = settings.reference();
                settings.observe(&[summary(4)], true);
                assert!(!settings.invalid, "adopts observed revisions");
                assert_eq!(
                    settings.reference(),
                    summary(4).reference(),
                    "adopts observed revisions"
                );
                assert!(
                    settings.begin(RequestId::new(), &previous).is_err(),
                    "adopts observed revisions"
                );
                settings
                    .begin(RequestId::new(), &settings.reference())
                    .unwrap();
            }),
        ),
        (
            "invalidates_conflicting_revisions",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                settings.observe(&[summary(4)], true);
                settings.failed(id);
                assert!(settings.invalid, "invalidates conflicting revisions");
                assert!(
                    settings.pending.is_none(),
                    "invalidates conflicting revisions"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    3,
                    "invalidates conflicting revisions"
                );
            }),
        ),
        (
            "same_revision_failure_preserves_the_editable_draft",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                settings.failed(id);
                assert!(
                    !settings.invalid,
                    "same revision failure preserves the editable draft"
                );
                assert!(
                    settings.pending.is_none(),
                    "same revision failure preserves the editable draft"
                );
            }),
        ),
        (
            "ignores_stale_metadata",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                assert!(
                    settings.finish(id, &summary(4).reference()),
                    "ignores stale metadata"
                );
                settings.observe(&[summary(3)], true);
                let next = RequestId::new();
                settings.begin(next, &settings.reference()).unwrap();
                settings.failed(next);
                assert!(!settings.invalid, "ignores stale metadata");
                assert_eq!(
                    settings.live.as_ref().unwrap().settings_revision,
                    3,
                    "ignores stale metadata"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    4,
                    "ignores stale metadata"
                );
            }),
        ),
        (
            "pending_save_never_adopts_unconfirmed_metadata",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                settings.observe(&[summary(4)], true);
                assert_eq!(
                    settings.pending.as_ref().unwrap().id,
                    id,
                    "pending save never adopts unconfirmed metadata"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    3,
                    "pending save never adopts unconfirmed metadata"
                );
                settings.observe(&[summary(5)], true);
                assert!(
                    settings.invalid,
                    "pending save never adopts unconfirmed metadata"
                );
            }),
        ),
        (
            "excludes_other_receipts_and_enablement_changes",
            (|_scenario: &str| {
                let mut settings = settings();
                settings
                    .begin(RequestId::new(), &settings.reference())
                    .unwrap();
                assert!(
                    !settings.finish(RequestId::new(), &summary(4).reference()),
                    "excludes other receipts and enablement changes"
                );
                assert!(
                    settings.invalid,
                    "excludes other receipts and enablement changes"
                );

                let mut settings = self::settings();
                settings
                    .begin(RequestId::new(), &settings.reference())
                    .unwrap();
                let mut live = summary(4);
                live.enabled = false;
                settings.observe(&[live], true);
                assert!(
                    settings.invalid,
                    "excludes other receipts and enablement changes"
                );
            }),
        ),
        (
            "rejects_arbitrary_provenance_and_disconnects",
            (|_scenario: &str| {
                let mut settings = settings();
                assert!(
                    settings
                        .begin(RequestId::new(), &summary(4).reference())
                        .is_err(),
                    "rejects arbitrary provenance and disconnects"
                );
                settings.observe(&[summary(3)], false);
                assert!(
                    settings.invalid,
                    "rejects arbitrary provenance and disconnects"
                );
            }),
        ),
        (
            "confirmed_receipts_survive_later_observations",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                settings.observe(&[summary(5)], true);
                assert!(
                    settings.invalid,
                    "confirmed receipts survive later observations"
                );
                assert!(
                    settings.finish(id, &summary(4).reference()),
                    "confirmed receipts survive later observations"
                );
                assert_eq!(
                    settings.reference().settings_revision,
                    4,
                    "confirmed receipts survive later observations"
                );
                assert!(
                    settings.pending.is_none(),
                    "confirmed receipts survive later observations"
                );
                assert!(
                    settings.invalid,
                    "confirmed receipts survive later observations"
                );
            }),
        ),
        (
            "confirms_without_an_inventory_round_trip",
            (|_scenario: &str| {
                let mut settings = settings();
                let id = RequestId::new();
                settings.begin(id, &settings.reference()).unwrap();
                assert!(
                    settings.finish(id, &summary(4).reference()),
                    "confirms without an inventory round trip"
                );
                assert_eq!(
                    settings.reference(),
                    summary(4).reference(),
                    "confirms without an inventory round trip"
                );
                assert!(
                    !settings.invalid,
                    "confirms without an inventory round trip"
                );
                settings.observe(&[summary(4)], true);
                let next = RequestId::new();
                settings.begin(next, &settings.reference()).unwrap();
                assert!(
                    settings.finish(next, &summary(5).reference()),
                    "confirms without an inventory round trip"
                );
                assert_eq!(
                    settings.reference(),
                    summary(5).reference(),
                    "confirms without an inventory round trip"
                );
            }),
        ),
        (
            "confirms_serial_saves_before_inventory_updates",
            (|_scenario: &str| {
                let mut settings = settings();
                for revision in [4, 5] {
                    let id = RequestId::new();
                    settings.begin(id, &settings.reference()).unwrap();
                    assert!(
                        settings.finish(id, &summary(revision).reference()),
                        "confirms serial saves before inventory updates"
                    );
                    assert!(
                        settings.pending.is_none(),
                        "confirms serial saves before inventory updates"
                    );
                }
                settings.observe(&[summary(4)], true);
                assert_eq!(
                    settings.reference(),
                    summary(5).reference(),
                    "confirms serial saves before inventory updates"
                );
                assert!(
                    !settings.invalid,
                    "confirms serial saves before inventory updates"
                );
                let pending = RequestId::new();
                settings.begin(pending, &settings.reference()).unwrap();
                assert_eq!(
                    settings.pending.as_ref().unwrap().id,
                    pending,
                    "confirms serial saves before inventory updates"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
