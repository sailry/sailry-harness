use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Command, Output};
use std::sync::atomic::Ordering;
mod fixture;
use fixture::{Fixture, choose, copied, draw, fill, init, input, shown, tap, wait};

#[track_caller]
fn toast(visual: &mut VisualTestContext, expected: &str) {
    crate::feedback::tests::shown(visual);
    assert_eq!(visual.update(crate::feedback::tests::summary), expected);
}

#[gpui::test]
fn saves_to_captured_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "role-add");
        fill(visual, "review");
        input(visual, "role-instructions", "Inspect changed files");
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "role-save");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        fixture.bind(&owner, visual, true);
        fixture.transport.release.cancel();
        wait(visual, |cx| owner.read(cx).roles.is_empty());
        let saved = fixture.roles();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].instructions, "Inspect changed files");
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::Roles(roles) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ListRoles)))
            .unwrap()
        else {
            panic!("roles expected");
        };
        assert!(roles.is_empty());
        fixture.bind(&owner, visual, false);
        wait(visual, |cx| owner.read(cx).roles.len() == 1);
        shown(&owner, visual, "role-settings", true);
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin {
            name: "delegation".into(),
        }) else {
            panic!("package expected");
        };
        fixture.execute(Command::SetPluginEnabled {
            name: "delegation".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        // Core composer references do not disappear with the settings/tool package.
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "delegation" && !package.enabled)
        });
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.role_profiles().len()),
            1
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_conflicting_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        assert!(owner.read_with(visual, |owner, _| owner.roles.is_empty()));
        tap(visual, "role-add");
        tap(visual, "role-save");
        toast(visual, "Check the identifier, name and turn limit");
        assert!(visual.debug_bounds("role-error").is_none());
        fill(visual, "review");
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        wait(visual, |cx| owner.read(cx).roles.len() == 1);
        let original = fixture.roles().remove(0);
        assert_eq!(original.name, "Review 中文 🙂");
        assert!(original.model.is_none());
        tap(visual, &format!("role-edit-{}", fixture.roles()[0].id));
        input(visual, "role-field-role_id", "review-renamed");
        choose(visual, "role-source", 1);
        choose(visual, "role-effort", 0);
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        wait(visual, |_| fixture.roles()[0].revision == 2);
        let saved = fixture.roles().remove(0);
        assert_eq!(saved.id, original.id);
        assert_eq!(saved.key, "review-renamed");
        assert_eq!(saved.model.as_ref().unwrap().provider, fixture.provider.id);
        assert_eq!(saved.model.as_ref().unwrap().effort, None);

        tap(visual, &format!("role-edit-{}", fixture.roles()[0].id));
        input(visual, "role-field-settings_name", "Preserved draft 中文");
        let mut other = saved.clone();
        other.name = "Changed elsewhere".into();
        fixture.execute(Command::PutRole {
            role: other,
            expected_revision: 2,
        });
        wait(visual, |_| fixture.roles()[0].revision == 3);
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "role-save");
        toast(visual, "Subagent changed; reopen it");
        assert!(visual.debug_bounds("role-error").is_none());
        tap(visual, "role-field-settings_name");
        assert_eq!(copied(visual), "Preserved draft 中文");
        assert_eq!(fixture.roles()[0].name, "Changed elsewhere");
        tap(visual, "role-cancel");
        tap(visual, &format!("role-delete-{}", fixture.roles()[0].id));
        let mut other = fixture.roles().remove(0);
        other.name = "Latest role".into();
        fixture.execute(Command::PutRole {
            role: other,
            expected_revision: 3,
        });
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "role-delete-confirm");
        toast(visual, "Subagent changed; reopen it");
        shown(&owner, visual, "role-removal-error", false);
        shown(&owner, visual, "role-removal", true);
        assert_eq!(fixture.roles()[0].name, "Latest role");
        tap(visual, "role-delete-cancel");
        wait(visual, |_| fixture.roles()[0].revision == 4);
        tap(visual, &format!("role-delete-{}", fixture.roles()[0].id));
        tap(visual, "role-delete-confirm");
        assert!(!visual.has_pending_prompt());
        wait(visual, |cx| owner.read(cx).roles.is_empty());
        assert!(fixture.roles().is_empty());
        fixture.close(visual);
    }
}

#[gpui::test]
fn retries_bound_requests(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "role-add");
        fill(visual, "review");
        choose(visual, "role-source", 1);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "role-save");
        toast(visual, "Result unconfirmed; retry the original request");
        assert!(visual.debug_bounds("role-error").is_none());
        assert_eq!(fixture.roles().len(), 1);
        input(visual, "role-field-settings_name", "Ignored pending edit");
        assert_eq!(copied(visual), "Review 中文 🙂");
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        assert_eq!(fixture.roles()[0].revision, 1);
        {
            let requests = fixture.transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], requests[1]);
        }
        fixture.bind(&owner, visual, false);
        wait(visual, |cx| owner.read(cx).roles.len() == 1);
        tap(visual, &format!("role-delete-{}", fixture.roles()[0].id));
        fixture.transport.mode.store(1, Ordering::SeqCst);
        let mut replacement = fixture.roles().remove(0);
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "role-delete-confirm");
        toast(visual, "Result unconfirmed; retry the original request");
        shown(&owner, visual, "role-removal-error", false);
        shown(&owner, visual, "role-removal", true);
        assert!(fixture.roles().is_empty());
        replacement.id = sailry_protocol::RoleId::new();
        let expected = replacement.id;
        fixture.execute(Command::PutRole {
            role: replacement,
            expected_revision: 0,
        });
        tap(visual, "role-delete-confirm");
        assert!(!visual.has_pending_prompt());
        assert_eq!(fixture.roles()[0].id, expected);
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2], requests[3]);
        drop(requests);
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_replacement_dialog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "role-add");
        fill(visual, "review");
        fixture.transport.mode.store(2, Ordering::SeqCst);
        tap(visual, "role-save");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert_eq!(fixture.roles().len(), 1);
        tap(visual, "role-cancel");
        tap(visual, "role-add");
        fill(visual, "new-draft");
        fixture.transport.release.cancel();
        // Let the already completed Node receipt return to the retired editor.
        for _ in 0..20 {
            draw(visual);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        shown(&owner, visual, "role-editor", true);
        tap(visual, "role-field-role_id");
        assert_eq!(copied(visual), "new-draft");
        assert_eq!(fixture.roles().len(), 1);
        tap(visual, "role-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_missing_model_selection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let role = sailry_protocol::role::Profile {
            id: sailry_protocol::RoleId::new(),
            revision: 0,
            key: "review".into(),
            name: "Review 中文".into(),
            appearance: None,
            description: "Inspect changes".into(),
            model: Some(sailry_protocol::role::Model {
                provider: fixture.provider.id,
                model: "fixture-model".into(),
                effort: Some(sailry_protocol::Effort::High),
            }),
            max_turns: Some(12),
            skills: vec!["external-review".into()],
            instructions: "Keep these instructions 中文 🙂\nDo not change files".into(),
        };
        fixture.execute(Command::PutRole {
            role: role.clone(),
            expected_revision: 0,
        });
        fixture.execute(Command::RemoveProvider {
            provider: fixture.provider.id,
            expected_revision: 1,
        });
        let (owner, visual) = fixture.mount(cx);
        wait(visual, |cx| owner.read(cx).roles.len() == 1);
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.roles[0].model.clone()),
            Some((usize::MAX, "fixture-model".into()))
        );
        tap(visual, &format!("role-edit-{}", fixture.roles()[0].id));
        shown(&owner, visual, "role-model", true);
        input(
            visual,
            "role-field-settings_name",
            "Saved after explicit inheritance",
        );
        tap(visual, "role-save");
        toast(visual, "Selected model or provider is unavailable");
        assert!(visual.debug_bounds("role-error").is_none());
        assert_eq!(fixture.roles()[0].revision, 1);
        choose(visual, "role-source", 0);
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        let saved = fixture.roles().remove(0);
        assert_eq!(saved.revision, 2);
        assert!(saved.model.is_none());
        assert_eq!(saved.skills, role.skills);
        assert_eq!(saved.description, role.description);
        assert_eq!(saved.instructions, role.instructions);
        assert_eq!(saved.max_turns, Some(12));
        fixture.close(visual);
    }
}

#[gpui::test]
fn presets(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "role-add");
        fill(visual, "review");
        assert!(visual.debug_bounds("role-description").is_none());
        assert!(visual.debug_bounds("role-skill-0").is_none());
        tap(visual, "role-appearance");
        tap(visual, "role-color-violet");
        tap(visual, "role-icon-code");
        tap(visual, "role-field-role_id");
        choose(visual, "role-presets", 0);
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        wait(visual, |cx| owner.read(cx).roles.len() == 1);
        let saved = fixture.roles().remove(0);
        assert!(saved.description.is_empty());
        assert!(saved.skills.is_empty());
        assert_eq!(saved.instructions, fixture::preset("review"));
        assert_eq!(
            saved.appearance,
            Some(sailry_protocol::projects::Appearance {
                icon: "code".into(),
                color: "violet".into(),
            })
        );
        tap(visual, &format!("role-edit-{}", fixture.roles()[0].id));
        choose(visual, "role-presets", 1);
        tap(visual, "role-save");
        shown(&owner, visual, "role-editor", false);
        wait(visual, |_| fixture.roles()[0].revision == 2);
        assert_eq!(fixture.roles()[0].instructions, fixture::preset("research"));
        assert_eq!(fixture.roles()[0].appearance, saved.appearance);
        fixture.close(visual);
    }
}
