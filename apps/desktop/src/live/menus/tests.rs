use super::*;
use core::prelude::v1::test;

#[test]
fn limits_host_actions() {
    for local in [true, false] {
        assert!(items(Target::Host, local).contains(&(Command::Edit, "connection_action_edit")));
        assert_eq!(
            items(Target::Project(ProjectId::new()), local),
            vec![
                (Command::Open, "workspace_project"),
                (Command::Edit, "project_edit"),
                (Command::NewSession, "workspace_new_session"),
                (Command::Copy, "workspace_copy_path"),
                (Command::Remove, "project_remove"),
            ]
        );
    }
    assert!(
        !items(Target::Host, true)
            .iter()
            .any(|(action, _)| *action == Command::Revoke)
    );
    assert!(
        items(Target::Host, false)
            .iter()
            .any(|(action, _)| *action == Command::Revoke)
    );
    assert!(
        items(Target::Session(SessionId::new()), true)
            .contains(&(Command::Delete, "session_delete"))
    );
}
