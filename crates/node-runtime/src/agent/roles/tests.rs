use super::*;

const SESSION: &str = "0f0e0d0c-0b0a-4908-8706-050403020100";

fn profile(revision: u64) -> role::Profile {
    role::Profile {
        id: RoleId::new(),
        revision,
        key: "review".into(),
        name: "Review".into(),
        appearance: None,
        description: String::new(),
        read_only: true,
        model: None,
        max_turns: Some(3),
        skills: vec![],
        tools: None,
        instructions: "Inspect".into(),
    }
}

fn team(revision: u64, members: Vec<RoleId>) -> team::Profile {
    team::Profile {
        id: TeamId::new(),
        revision,
        key: "backend".into(),
        name: "Backend".into(),
        appearance: None,
        description: String::new(),
        model: None,
        instructions: "Coordinate".into(),
        members,
    }
}

fn invalid(tool: Tool, arguments: Value, current: Option<Output>) -> ErrorCode {
    command(tool, SESSION, arguments, current).unwrap_err().code
}

mod agents {
    use super::*;

    #[test]
    fn adds_with_defaults() {
        let Command::PutRole {
            role,
            expected_revision: 0,
        } = command(
            Tool::AddAgent,
            SESSION,
            json!({"key":"writer","name":"Writer","instructions":"Write"}),
            None,
        )
        .unwrap()
        else {
            panic!("new role expected")
        };
        assert_eq!((role.revision, role.key.as_str()), (0, "writer"));
        assert!(!role.read_only && role.skills.is_empty() && role.model.is_none());
        for arguments in [
            json!({"key":"writer","name":"Writer"}),
            json!({"key":"writer","name":"Writer","instructions":"Write","id":"x"}),
            json!({"key":"Writer","name":"Writer","instructions":"Write"}),
        ] {
            assert_eq!(
                invalid(Tool::AddAgent, arguments, None),
                ErrorCode::InvalidRequest
            );
        }
    }

    #[test]
    fn edits_the_exact_revision() {
        let current = profile(9007199254740993);
        let catalog = || Some(Output::Roles(vec![current.clone()]));
        let arguments = json!({"id":current.id,"expected_revision":"9007199254740993","name":"Edited","max_turns":null});
        let Command::PutRole {
            role,
            expected_revision,
        } = command(Tool::EditAgent, SESSION, arguments, catalog()).unwrap()
        else {
            panic!("edited role expected")
        };
        assert_eq!(expected_revision, 9007199254740993);
        assert_eq!(role.name, "Edited");
        assert_eq!(role.max_turns, None);
        assert_eq!(role.instructions, current.instructions);
        assert_eq!(
            invalid(
                Tool::EditAgent,
                json!({"id":current.id,"expected_revision":"1","name":"Stale"}),
                catalog()
            ),
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            invalid(
                Tool::EditAgent,
                json!({"id":RoleId::new(),"expected_revision":"1","name":"Missing"}),
                catalog()
            ),
            ErrorCode::NotFound
        );
        for arguments in [
            json!({"id":current.id,"expected_revision":"1"}),
            json!({"id":current.id,"expected_revision":"0","name":"Zero"}),
            json!({"id":current.id,"expected_revision":"01","name":"Padded"}),
            json!({"id":current.id,"expected_revision":1,"name":"Number"}),
            json!({"id":current.id,"expected_revision":"9223372036854775808","name":"Large"}),
        ] {
            assert_eq!(
                invalid(Tool::EditAgent, arguments, catalog()),
                ErrorCode::InvalidRequest
            );
        }
    }

    #[test]
    fn applies_distinct_references() {
        let id = RoleId::new();
        let Command::SetSessionRoles {
            session,
            expected_revision: 4,
            roles,
        } = command(
            Tool::ApplyAgents,
            SESSION,
            json!({"agents":[{"id":id,"revision":"2"}],"expected_session_revision":"4"}),
            None,
        )
        .unwrap()
        else {
            panic!("roster application expected")
        };
        assert_eq!(session.to_string(), SESSION);
        assert_eq!(roles, vec![role::Reference { id, revision: 2 }]);
        assert_eq!(
            invalid(
                Tool::ApplyAgents,
                json!({"agents":[{"id":id,"revision":"2"},{"id":id,"revision":"2"}],"expected_session_revision":"4"}),
                None
            ),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            invalid(
                Tool::ApplyAgents,
                json!({"agents":[],"expected_session_revision":"4","session":SESSION}),
                None
            ),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn projects_decimal_revisions() {
        let role = profile(9007199254740993);
        let listed = project(Tool::ListAgents, Output::Roles(vec![role.clone()])).unwrap();
        assert_eq!(listed["agents"][0]["revision"], "9007199254740993");
        let saved = project(Tool::EditAgent, Output::Role(role)).unwrap();
        assert_eq!(saved["revision"], "9007199254740993");
        assert!(project(Tool::ListAgents, Output::Teams(vec![])).is_err());
    }
}

mod teams {
    use super::*;

    #[test]
    fn adds_and_edits_members() {
        let member = RoleId::new();
        let Command::PutTeam {
            team: added,
            expected_revision: 0,
        } = command(
            Tool::AddTeam,
            SESSION,
            json!({"key":"backend","name":"Backend","members":[member]}),
            None,
        )
        .unwrap()
        else {
            panic!("new team expected")
        };
        assert_eq!(added.members, vec![member]);
        assert_eq!(
            invalid(
                Tool::AddTeam,
                json!({"key":team::DEFAULT,"name":"Default"}),
                None
            ),
            ErrorCode::InvalidRequest
        );
        let current = team(3, vec![member]);
        let Command::PutTeam {
            team: edited,
            expected_revision: 3,
        } = command(
            Tool::EditTeam,
            SESSION,
            json!({"id":current.id,"expected_revision":"3","members":[]}),
            Some(Output::Teams(vec![current.clone()])),
        )
        .unwrap()
        else {
            panic!("edited team expected")
        };
        assert!(edited.members.is_empty());
        assert_eq!(edited.instructions, current.instructions);
        assert_eq!(
            invalid(
                Tool::EditTeam,
                json!({"id":current.id,"expected_revision":"2","name":"Stale"}),
                Some(Output::Teams(vec![current]))
            ),
            ErrorCode::RevisionConflict
        );
    }

    #[test]
    fn applies_a_team_or_the_default() {
        let id = TeamId::new();
        let Command::SetSessionTeam {
            expected_revision: 5,
            team: Some(selected),
            ..
        } = command(
            Tool::ApplyTeam,
            SESSION,
            json!({"team":{"id":id,"revision":"2"},"expected_session_revision":"5"}),
            None,
        )
        .unwrap()
        else {
            panic!("team application expected")
        };
        assert_eq!(selected, team::Reference { id, revision: 2 });
        assert!(matches!(
            command(
                Tool::ApplyTeam,
                SESSION,
                json!({"team":null,"expected_session_revision":"5"}),
                None
            )
            .unwrap(),
            Command::SetSessionTeam { team: None, .. }
        ));
        assert_eq!(
            invalid(
                Tool::ApplyTeam,
                json!({"expected_session_revision":"5"}),
                None
            ),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn removes_with_exact_revision() {
        let id = TeamId::new();
        assert!(matches!(
            command(
                Tool::DeleteTeam,
                SESSION,
                json!({"id":id,"expected_revision":"7"}),
                None
            )
            .unwrap(),
            Command::RemoveTeam { team, expected_revision: 7 } if team == id
        ));
    }
}

#[test]
fn declarations_cover_every_tool() {
    let names: Vec<_> = TOOLS.iter().map(|tool| tool.name()).collect();
    assert_eq!(names, NAMES);
    for tool in TOOLS {
        let schema = tool.parameters();
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{}",
            tool.name()
        );
        assert_eq!(
            tool.read_only(),
            matches!(tool, Tool::ListAgents | Tool::ListTeams)
        );
        assert!(tool.display().is_some());
    }
    assert!(APPLY.iter().all(|name| NAMES.contains(name)));
}
