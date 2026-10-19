use super::*;
use sailry_protocol::plugin::{McpTransport, conversation::Resource};

impl Details {
    pub(super) fn external_contents(&self, cx: &App) -> AnyElement {
        use sailry_protocol::plugin::catalog::{Details as CatalogDetails, Format};
        let mut view = v_flex().gap_5();
        let (source, formats) = match self.external.as_ref() {
            Some(CatalogDetails::Skills(collection)) => {
                let skills = collection
                    .skills
                    .iter()
                    .map(|candidate| {
                        rows::row(
                            format!("catalog-skill-{}", candidate.name),
                            IconName::BookOpen,
                            candidate
                                .skill
                                .display_name(&rust_i18n::locale())
                                .to_owned(),
                            Some(candidate.path.clone()),
                            div(),
                            cx,
                        )
                        .into_any_element()
                    })
                    .collect();
                view = view.child(rows::section(
                    "market_details_skills",
                    Some(collection.skills.len()),
                    skills,
                    cx,
                ));
                (&collection.source, collection.unsupported.as_slice())
            }
            Some(CatalogDetails::Unsupported(external)) => {
                (&external.source, external.formats.as_slice())
            }
            _ => return div().into_any_element(),
        };
        if !formats.is_empty() {
            let formats = formats
                .iter()
                .enumerate()
                .map(|(index, format)| {
                    rows::row(
                        format!("catalog-format-{index}"),
                        IconName::Package,
                        match format {
                            Format::Codex => "Codex",
                            Format::ClaudeCode => "Claude Code",
                        },
                        None,
                        rows::tag(tr("market_details_unsupported")),
                        cx,
                    )
                    .into_any_element()
                })
                .collect();
            view = view.child(rows::section("market_details_formats", None, formats, cx));
        }
        if self.tab == Tab::Overview {
            let metadata = self.external_metadata();
            let mut details = vec![
                rows::row(
                    "catalog-source".into(),
                    IconName::Globe,
                    tr("market_details_source"),
                    None,
                    source.repository.clone(),
                    cx,
                )
                .into_any_element(),
            ];
            if let Some(version) = metadata.and_then(|metadata| metadata.version.as_ref()) {
                details.push(
                    rows::row(
                        "catalog-version".into(),
                        IconName::Tag,
                        tr("market_details_version"),
                        None,
                        version.clone(),
                        cx,
                    )
                    .into_any_element(),
                );
            }
            view = view.child(rows::section(
                "market_details_information",
                None,
                details,
                cx,
            ));
        }
        view.into_any_element()
    }
}

impl Details {
    pub(super) fn contents(&self, info: &Info, cx: &App) -> AnyElement {
        let locale = rust_i18n::locale();
        let mut body = v_flex().gap_5();
        if let Some(extension) = &info.extension
            && !extension.tools.is_empty()
        {
            let tools = extension
                .tools
                .iter()
                .map(|tool| {
                    let title = tool
                        .display
                        .as_ref()
                        .map(|display| display.label(&locale).to_owned())
                        .unwrap_or_else(|| tool.name.clone());
                    let mut tags = h_flex().gap_1().flex_wrap();
                    for context in &tool.contexts {
                        let key = match context {
                            Resource::Plugin => "market_details_context_plugin",
                            Resource::Workspace => "market_details_context_workspace",
                            Resource::Database => "market_details_context_database",
                            Resource::Ssh => "market_details_context_ssh",
                        };
                        tags = tags.child(rows::tag(tr(key)));
                    }
                    if tool.server.is_some() {
                        tags = tags.child(rows::tag("MCP"));
                    }
                    rows::row(
                        format!("plugin-tool-{}", tool.name),
                        IconName::Wrench,
                        title,
                        tool.display.as_ref().map(|_| tool.name.clone()),
                        tags,
                        cx,
                    )
                    .when(tool.display.is_none(), |row| {
                        row.font_family(cx.theme().mono_font_family.clone())
                    })
                    .into_any_element()
                })
                .collect();
            body = body.child(rows::section(
                "market_details_tools",
                Some(extension.tools.len()),
                tools,
                cx,
            ));
        }
        if !info.skills.is_empty() {
            let skills = info
                .skills
                .iter()
                .map(|skill| {
                    rows::row(
                        format!("plugin-skill-{}", skill.name),
                        IconName::BookOpen,
                        skill.display_name(&locale).to_owned(),
                        Some(skill.name.clone()),
                        div(),
                        cx,
                    )
                    .into_any_element()
                })
                .collect();
            body = body.child(rows::section(
                "market_details_skills",
                Some(info.skills.len()),
                skills,
                cx,
            ));
        }
        if !info.mcp.is_empty() {
            let servers = info
                .mcp
                .iter()
                .map(|server| {
                    let transport = tr(match server.transport {
                        McpTransport::Stdio => "plugins_stdio",
                        McpTransport::StreamableHttp => "plugins_http",
                        McpTransport::Sse => "plugins_sse",
                    });
                    let subtitle = server.host.as_ref().map_or_else(
                        || transport.to_string(),
                        |host| format!("{transport} · {host}"),
                    );
                    let allowed = matches!(self.target, Target::Installed(_))
                        && self
                            .authorization
                            .read(cx)
                            .allows(&info.summary.reference(), &server.name);
                    let action = div().when(allowed, |view| {
                        view.child(
                            authorization::action(
                                self.binding.clone(),
                                info.summary.reference(),
                                server.name.clone(),
                                format!("plugin-mcp-auth-{}", server.name),
                            )
                            .disabled(self.pending(cx)),
                        )
                    });
                    rows::row(
                        format!("plugin-mcp-{}", server.name),
                        IconName::Server,
                        server.name.clone(),
                        Some(subtitle),
                        action,
                        cx,
                    )
                    .font_family(cx.theme().mono_font_family.clone())
                    .into_any_element()
                })
                .collect();
            body = body.child(rows::section(
                "market_details_mcp",
                Some(info.mcp.len()),
                servers,
                cx,
            ));
        }
        let issues: Vec<_> = info
            .issues
            .iter()
            .filter(|issue| {
                !matches!(
                    issue.kind,
                    IssueKind::InvalidSkills | IssueKind::InvalidSkill
                )
            })
            .map(|issue| {
                rows::row(
                    format!("plugin-issue-{}-{}", issue_key(issue.kind), issue.path),
                    IconName::CircleAlert,
                    tr(issue_key(issue.kind)),
                    Some(issue.path.clone()),
                    div(),
                    cx,
                )
                .into_any_element()
            })
            .collect();
        if !issues.is_empty() {
            body = body.child(rows::section(
                "plugins_issues",
                Some(issues.len()),
                issues,
                cx,
            ));
        }
        body.into_any_element()
    }
}
