use super::*;
use gpui_kit::component::tag::Tag;
use sailry_protocol::plugin::{Access, Action};

pub(super) fn label(action: &Action) -> &'static str {
    match action {
        Action::ReadArtifacts => "market_permission_artifacts_read",
        Action::WriteArtifacts => "market_permission_artifacts_write",
        Action::ReadCodeIndex | Action::ReadCode => "market_permission_code_read",
        Action::ManageCode => "market_permission_code_manage",
        Action::ReadCodeIndexSettings | Action::ReadCodeSettings => {
            "market_permission_code_settings_read"
        }
        Action::SaveCodeIndexSettings | Action::SaveCodeSettings => {
            "market_permission_code_settings_write"
        }
        Action::ReadSearchSettings => "market_permission_web_settings_read",
        Action::SaveSearchSettings => "market_permission_web_settings_write",
        Action::ReadActivity => "market_permission_activity_read",
        Action::ReadUsage => "market_permission_usage_read",
        Action::ReadDatabases => "market_permission_databases_read",
        Action::ControlDatabases => "market_permission_databases_control",
        Action::ManageDatabases => "market_permission_databases_manage",
        Action::ReadSsh => "market_permission_ssh_read",
        Action::ControlSsh => "market_permission_ssh_control",
        Action::ManageSsh => "market_permission_ssh_manage",
        Action::DelegateAgents => "market_permission_agents_delegate",
        Action::ReadRoles => "market_permission_roles_read",
        Action::WriteRoles => "market_permission_roles_write",
        Action::ReadModels => "market_permission_models_read",
        Action::RunDecisions => "market_permission_decisions_run",
        Action::ProvideModels => "market_permission_models_provide",
        Action::AuthenticateModels => "market_permission_models_authenticate",
        Action::InspectCredentials => "market_permission_credentials_inspect",
        Action::InstallModels => "market_permission_models_install",
        Action::InspectMedia => "market_permission_media_inspect",
        Action::GenerateImage => "market_permission_media_image",
        Action::GenerateVideo => "market_permission_media_video",
        Action::ReadMediaSettings => "market_permission_media_settings_read",
        Action::SaveMediaSettings => "market_permission_media_settings_write",
        Action::ReadComputer => "market_permission_computer_read",
        Action::ControlComputer => "market_permission_computer_control",
        Action::ReadBrowser => "market_permission_browser_read",
        Action::ControlBrowser => "market_permission_browser_control",
        Action::ReadExternalBrowser => "market_permission_external_browser_read",
        Action::ControlExternalBrowser => "market_permission_external_browser_control",
        Action::Notify => "market_permission_notifications_publish",
        Action::Dispatch => "market_permission_dispatch_manage",
        Action::ReadFiles => "market_permission_files_read",
        Action::WriteFiles => "market_permission_files_write",
        Action::ReadGit => "market_permission_git_read",
        Action::WriteGit => "market_permission_git_write",
        Action::ReadWorktrees => "market_permission_worktrees_read",
        Action::WriteWorktrees => "market_permission_worktrees_write",
        Action::ReadCommands => "market_permission_process_read",
        Action::ControlCommands => "market_permission_process_control",
        Action::ReadTerminals => "market_permission_terminals_read",
        Action::ControlTerminals => "market_permission_terminals_control",
        Action::ReadConversation => "market_permission_conversation_read",
        Action::ControlConversation => "market_permission_conversation_control",
        Action::StartSessions => "market_permission_sessions_start",
        Action::ReadProjects => "market_permission_projects_read",
        Action::Http => "market_permission_http_request",
        Action::ReadWeb => "market_permission_web_read",
        Action::GenerateText => "market_permission_models_generate",
        Action::ReadStorage => "market_permission_storage_read",
        Action::WriteStorage => "market_permission_storage_write",
        Action::ReadKnowledge => "market_permission_knowledge_read",
        Action::WriteKnowledge => "market_permission_knowledge_write",
    }
}

pub(super) fn raw(action: &Action) -> String {
    serde_json::to_value(action)
        .expect("action serialization")
        .as_str()
        .expect("action name")
        .to_owned()
}

fn icon(action: &Action) -> IconName {
    let name = raw(action);
    match name.split('.').next().unwrap_or_default() {
        "files" | "artifacts" => IconName::File,
        "storage" | "databases" | "knowledge" => IconName::Database,
        "agents" | "roles" | "sessions" | "conversation" => IconName::Users,
        "process" | "terminals" | "ssh" => IconName::SquareTerminal,
        "git" | "code" | "models" => IconName::Code,
        "worktrees" | "projects" => IconName::Folder,
        "web" | "browser" | "external_browser" | "http" => IconName::Globe,
        _ => IconName::Shield,
    }
}

impl Details {
    pub(super) fn permissions(&self, info: &Info, cx: &App) -> AnyElement {
        let actions = info
            .extension
            .as_ref()
            .map(|value| value.actions.as_slice())
            .unwrap_or_default();
        let mut items = Vec::new();
        let local_commands = info
            .extension
            .iter()
            .flat_map(|extension| &extension.connectors)
            .flat_map(|connector| &connector.auth)
            .any(|auth| {
                matches!(
                    auth,
                    sailry_protocol::plugin::connector::Auth::LocalCli { .. }
                )
            });
        if actions.is_empty() && !local_commands {
            items.push(
                rows::row(
                    "plugin-no-permissions".into(),
                    IconName::Shield,
                    tr("market_details_permission_none"),
                    None,
                    div(),
                    cx,
                )
                .into_any_element(),
            );
        }
        for action in actions {
            let (tag, access) = match action.access() {
                Access::Read => (Tag::secondary(), "read"),
                Access::Modify => (Tag::warning().outline(), "modify"),
                Access::Operate => (Tag::warning().outline(), "operate"),
            };
            items.push(
                rows::row(
                    format!("plugin-permission-{}", raw(action)),
                    icon(action),
                    tr(label(action)),
                    Some(raw(action)),
                    tag.small().child(access),
                    cx,
                )
                .into_any_element(),
            );
        }
        for connector in info
            .extension
            .iter()
            .flat_map(|extension| &extension.connectors)
        {
            for auth in &connector.auth {
                if let sailry_protocol::plugin::connector::Auth::LocalCli { command, .. } = auth {
                    items.push(
                        rows::row(
                            format!("plugin-cli-{}", connector.id),
                            IconName::SquareTerminal,
                            tr("connector_cli_capability"),
                            Some(command.clone()),
                            Tag::warning().outline().small().child("operate"),
                            cx,
                        )
                        .into_any_element(),
                    );
                }
            }
        }
        let hosts: std::collections::BTreeSet<_> = info
            .mcp
            .iter()
            .filter_map(|mcp| mcp.host.as_deref())
            .collect();
        for host in hosts {
            items.push(
                rows::row(
                    format!("plugin-network-{host}"),
                    IconName::Globe,
                    tr("market_details_permission_network"),
                    Some(host.into()),
                    rows::tag("MCP"),
                    cx,
                )
                .into_any_element(),
            );
        }
        rows::section("market_details_permissions", None, items, cx)
    }
}
