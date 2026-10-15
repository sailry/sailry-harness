use crate::plugins::fixture::Fixture;
use sailry_protocol::{Command, Output, plugin::Scope};

#[test]
fn packages_are_not_turn_defaults() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let queue = || {
            let Output::Session(session) = fixture.execute(Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected");
            };
            let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect tools".into(),
            }) else {
                panic!("turn expected");
            };
            let mut packages = turn.plugins;
            packages.sort_by(|left, right| left.name.cmp(&right.name));
            packages
        };
        let baseline = queue();
        assert!(baseline.iter().any(|package| package.name == "reminders"));
        assert!(
            baseline
                .iter()
                .any(|package| package.name == "scheduled-tasks")
        );
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["scope"] = "desktop".into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        let installed = fixture.install(0);
        assert_eq!(installed.extension.unwrap().scope, Scope::Desktop);
        assert!(
            !baseline
                .iter()
                .any(|package| package.name == installed.summary.name)
        );
        assert_eq!(queue(), baseline);
        manifest["extensions"]["dev.sailry.platform"]["scope"] = "host".into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        let installed = fixture.install(installed.summary.revision);
        let mut expected = baseline;
        expected.push(installed.summary.reference());
        expected.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(queue(), expected);
        fixture.close();
    }
}
