use super::*;
use crate::settings::providers::Binding;
use crate::theme::DialogStyle as _;
use gpui_kit::component::skeleton::Skeleton;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::{Info, IssueKind, McpTransport, Summary, catalog::Source},
};

enum Target {
    Installed(Summary),
    Catalog {
        source: Source,
        id: String,
        name: String,
        bundled: bool,
    },
}

pub(super) struct Details {
    binding: Binding,
    target: Target,
    info: Option<Info>,
    error: Option<&'static str>,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn open(owner: Entity<Workspace>, expected: Summary, window: &mut Window, cx: &mut App) {
    let Some(live) = &owner.read(cx).provider_link else {
        return;
    };
    mount(
        live.binding.clone(),
        Target::Installed(expected),
        window,
        cx,
    );
}

pub(crate) fn open_bound(
    client: std::sync::Arc<sailry_client::Client>,
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    label: SharedString,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) {
    mount(
        Binding {
            client,
            runtime,
            label,
        },
        Target::Installed(expected),
        window,
        cx,
    );
}

pub(super) fn open_catalog(
    binding: Binding,
    source: Source,
    entry: sailry_protocol::plugin::catalog::Entry,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Details> {
    mount(
        binding,
        Target::Catalog {
            source,
            id: entry.id,
            name: entry.name,
            bundled: entry.bundled,
        },
        window,
        cx,
    )
}

fn mount(binding: Binding, target: Target, window: &mut Window, cx: &mut App) -> Entity<Details> {
    let details = cx.new(|_| Details {
        binding,
        target,
        info: None,
        error: None,
        stop: CancellationToken::new(),
        task: None,
    });
    details.update(cx, |_, cx| {
        crate::feedback::observe_with(
            window,
            cx,
            |view, _| view.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                gpui_kit::component::notification::Notification::error(tr(key)).action(
                    move |_, _, cx| {
                        let owner = owner.clone();
                        Button::new("plugin-details-retry")
                            .label(tr("plugins_retry"))
                            .debug_selector(|| "plugin-details-retry".into())
                            .on_click(cx.listener(move |toast, _, window, cx| {
                                toast.dismiss(window, cx);
                                _ = owner.update(cx, |view, cx| view.load(cx));
                            }))
                    },
                )
            },
        );
    });
    details.update(cx, |details, cx| details.load(cx));
    let entity = details.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = details.clone();
        dialog
            .form_title(
                h_flex()
                    .gap_3()
                    .pr_6()
                    .child(
                        div().min_w_0().truncate().child(
                            details
                                .read(cx)
                                .info
                                .as_ref()
                                .map(metadata::title)
                                .unwrap_or_else(|| details.read(cx).name().to_owned()),
                        ),
                    )
                    .when_some(details.read(cx).version(), |header, version| {
                        header.child(
                            div()
                                .text_sm()
                                .font_normal()
                                .text_color(cx.theme().muted_foreground)
                                .child(version),
                        )
                    }),
            )
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .on_close(move |_, _, cx| close.read(cx).stop.cancel())
            .child(details.clone())
    });
    entity
}

impl Details {
    fn name(&self) -> &str {
        match &self.target {
            Target::Installed(expected) => &expected.name,
            Target::Catalog { name, .. } => name,
        }
    }

    fn version(&self) -> Option<String> {
        self.info
            .as_ref()
            .and_then(|info| info.summary.version.clone())
            .or_else(|| {
                if let Target::Installed(expected) = &self.target {
                    expected.version.clone()
                } else {
                    None
                }
            })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() {
            return;
        }
        let binding = self.binding.clone();
        let command = match &self.target {
            Target::Installed(expected) => Command::ReadPlugin {
                name: expected.name.clone(),
            },
            Target::Catalog {
                source,
                id,
                bundled,
                ..
            } => Command::ReadCatalogPluginInfo {
                source: *source,
                id: id.clone(),
                bundled: *bundled,
            },
        };
        let stop = self.stop.clone();
        self.error = None;
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(command)) => Some(result),
            }
        });
        self.task = Some(cx.spawn(async move |details, cx| {
            let result = job.await;
            let _ = details.update(cx, |details, cx| {
                if details.stop.is_cancelled() {
                    return;
                }
                match result {
                    Ok(Some(Ok(Output::Plugin(info)))) => {
                        let matches = match &details.target {
                            Target::Installed(expected) => info.summary == *expected,
                            Target::Catalog { name, .. } => info.summary.name == *name,
                        };
                        if matches {
                            details.info = Some(info);
                        } else {
                            details.error = Some("plugins_conflict");
                        }
                    }
                    Ok(Some(Err(error))) => details.error = Some(live::error_key(&error)),
                    _ => details.error = Some("plugins_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().gap_4();
        if let Some(info) = &self.info {
            let installed = matches!(&self.target, Target::Installed(_));
            let configurable = info
                .settings
                .as_ref()
                .is_some_and(|schema| !schema.properties.is_empty());
            if installed && configurable {
                let binding = self.binding.clone();
                let expected = info.summary.clone();
                body = body.child(
                    Button::new("plugin-details-configure")
                        .label(tr("plugins_settings_title"))
                        .debug_selector(|| "plugin-details-configure".into())
                        .on_click(move |_, window, cx| {
                            configuration::mount(binding.clone(), expected.clone(), window, cx);
                        }),
                );
            }
            if installed
                && !info.mcp.is_empty()
                && (!configurable || !super::super::mcp::form_bound(info))
            {
                let binding = self.binding.clone();
                let info = info.clone();
                let id = if configurable {
                    "plugin-details-mcp-configure"
                } else {
                    "plugin-details-configure"
                };
                body = body.child(
                    Button::new(id)
                        .label(tr("mcp_edit"))
                        .debug_selector(move || id.into())
                        .on_click(move |_, window, cx| {
                            super::super::mcp::configure(binding.clone(), info.clone(), window, cx);
                        }),
                );
            }
            body = body.child(crate::plugins::emblem::render(
                &info.summary.name,
                crate::plugins::emblem::glyph(info),
                info.icon.as_deref(),
                px(48.),
                cx,
            ));
            body = body.when_some(metadata::description(info), |view, text| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(text),
                )
            });
            let mut skills = Group::new("skills_details");
            for skill in &info.skills {
                skills = skills.child(Entry::new(
                    format!("plugin-skill-{}", skill.name),
                    v_flex().gap_1().child(skill.name.clone()).child(
                        div()
                            .text_sm()
                            .whitespace_normal()
                            .text_color(cx.theme().muted_foreground)
                            .child(skill.description.clone()),
                    ),
                ));
            }
            let mut mcp = Group::new("mcp_servers");
            for server in &info.mcp {
                let binding = self.binding.clone();
                let package = info.summary.reference();
                let name = server.name.clone();
                mcp = mcp.child(
                    Entry::new(
                        format!("plugin-server-{}", server.name),
                        server.name.clone(),
                    )
                    .control(tr(match server.transport {
                        McpTransport::Stdio => "plugins_stdio",
                        McpTransport::StreamableHttp => "plugins_http",
                        McpTransport::Sse => "plugins_sse",
                    }))
                    .when(
                        installed && server.transport != McpTransport::Stdio,
                        |entry| {
                            entry.action(
                                Button::new(SharedString::from(format!("mcp-oauth-{name}")))
                                    .debug_selector({
                                        let name = name.clone();
                                        move || format!("plugin-authorize-{name}")
                                    })
                                    .label(tr("mcp_oauth_title"))
                                    .on_click(move |_, window, cx| {
                                        authorization::open(
                                            binding.clone(),
                                            package.clone(),
                                            name.clone(),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                        },
                    ),
                );
            }
            body = body
                .when(!info.skills.is_empty(), |view| view.child(skills))
                .when(!info.mcp.is_empty(), |view| view.child(mcp))
                .when(!info.mcp.is_empty(), |view| {
                    view.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("plugins_mcp_on_turn")),
                    )
                });
            if let Some(extension) = &info.extension {
                let mut tools = Group::new("plugins_tools");
                for tool in &extension.tools {
                    tools = tools.child(Entry::new(
                        format!("plugin-tool-{}", tool.name),
                        tool.name.clone(),
                    ));
                }
                body = body.when(!extension.tools.is_empty(), |view| view.child(tools));
            }
            body = body.child(
                Group::new("plugins_information")
                    .child(
                        Entry::new("plugin-info-name", tr("plugins_identifier"))
                            .control(info.summary.name.clone()),
                    )
                    .child(
                        Entry::new("plugin-info-version", tr("plugins_version"))
                            .control(info.summary.version.clone().unwrap_or_else(|| "—".into())),
                    ),
            );
            let visible_issues: Vec<_> = info
                .issues
                .iter()
                .filter(|issue| {
                    !matches!(
                        issue.kind,
                        IssueKind::InvalidSkill | IssueKind::InvalidSkills
                    )
                })
                .collect();
            if !visible_issues.is_empty() {
                let mut issues = Group::new("plugins_issues");
                for (index, issue) in visible_issues.iter().enumerate() {
                    issues = issues.child(Entry::new(
                        format!("plugin-issue-{index}"),
                        v_flex().gap_1().child(tr(issue_key(issue.kind))).child(
                            div()
                                .text_sm()
                                .whitespace_normal()
                                .text_color(cx.theme().muted_foreground)
                                .child(issue.path.clone()),
                        ),
                    ));
                }
                body = body.child(issues);
            }
        } else if self.error.is_none() {
            body = body.child(skeleton(cx));
        }
        body.id("plugin-live-details")
            .debug_selector(|| "plugin-live-details".into())
            // Kit's Dialog owns the single bounded scroll area.
            .flex_none()
            .p_1()
    }
}

fn skeleton(cx: &App) -> impl IntoElement {
    v_flex()
        .gap_4()
        .debug_selector(|| "plugin-details-skeleton".into())
        .child(Skeleton::new().size_12().rounded(cx.theme().radius_2xl()))
        .child(
            v_flex()
                .gap_2()
                .child(Skeleton::new().w_full().rounded_md())
                .child(Skeleton::new().w_2_3().rounded_md()),
        )
        .child(Skeleton::new().w_1_3().rounded_md())
        .child(
            Skeleton::new()
                .w_full()
                .h_20()
                .rounded(cx.theme().radius_lg),
        )
}

pub(super) fn issue_key(issue: IssueKind) -> &'static str {
    match issue {
        IssueKind::IgnoredManifestField => "plugins_issue_field",
        IssueKind::InvalidExtensions => "plugins_issue_extensions",
        IssueKind::UnsupportedExtension => "plugins_issue_extension_pending",
        IssueKind::UnavailablePath => "plugins_issue_path",
        IssueKind::InvalidSkills | IssueKind::InvalidSkill => "plugins_issue_skill",
        IssueKind::InvalidMcp | IssueKind::InvalidMcpServer => "plugins_issue_mcp",
    }
}
