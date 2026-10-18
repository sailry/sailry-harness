use super::*;
use crate::settings::providers::Binding;
use gpui_kit::assets::IconName;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::{Info, IssueKind, Summary, catalog::Source},
};

mod contents;
mod permissions;
mod rows;
mod versions;
mod view;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Overview,
    Contents,
    Permissions,
    Versions,
}

enum Target {
    Installed(Summary),
    Catalog {
        market: WeakEntity<market::Market>,
        entry: Box<sailry_protocol::plugin::catalog::Entry>,
        source: Source,
    },
}

pub(in crate::settings) struct Details {
    binding: Binding,
    target: Target,
    info: Option<Info>,
    authorization: Entity<authorization::Capabilities>,
    updates: Entity<updates::State>,
    tab: Tab,
    error: Option<&'static str>,
    stop: CancellationToken,
    task: Option<Task<()>>,
    removal: Option<Entity<dialog::Editor>>,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(in crate::settings) fn open(
    owner: Entity<Workspace>,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) {
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

pub(in crate::settings) fn open_catalog(
    market: Entity<market::Market>,
    binding: Binding,
    source: Source,
    entry: sailry_protocol::plugin::catalog::Entry,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Details> {
    mount(
        binding,
        Target::Catalog {
            market: market.downgrade(),
            entry: Box::new(entry),
            source,
        },
        window,
        cx,
    )
}

fn mount(binding: Binding, target: Target, window: &mut Window, cx: &mut App) -> Entity<Details> {
    let capabilities = cx.new(|_| authorization::Capabilities::new(binding.clone()));
    let update_state = updates::bind(binding.clone(), cx);
    let details = cx.new(|cx| {
        cx.observe(&update_state, |_, _, cx| cx.notify()).detach();
        cx.observe(&capabilities, |_, _, cx| cx.notify()).detach();
        Details {
            authorization: capabilities,
            updates: update_state,
            tab: Tab::Overview,
            binding,
            target,
            info: None,
            error: None,
            stop: CancellationToken::new(),
            task: None,
            removal: None,
        }
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
                        crate::feedback::retry("plugin-details-retry")
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
    window.open_sheet_at(Placement::Right, cx, move |sheet, window, cx| {
        let close = details.clone();
        sheet
            .size(px(480.).min(window.viewport_size().width))
            .resizable(false)
            .title(view::header(&details, cx))
            .on_close(move |_, _, cx| close.read(cx).stop.cancel())
            .child(details.clone())
    });
    entity
}

impl Details {
    fn configure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() || self.error.is_some() || self.pending(cx) {
            return;
        }
        let Target::Installed(expected) = &self.target else {
            return;
        };
        let Some(info) = self
            .info
            .clone()
            .filter(super::super::extensions::modal::configurable)
        else {
            return;
        };
        if info.summary.reference() != expected.reference() {
            return;
        }
        let binding = self.binding.clone();
        self.stop.cancel();
        window.close_sheet(cx);
        super::super::extensions::modal::open(binding, info, window, cx);
    }

    fn pending(&self, cx: &App) -> bool {
        self.updates.read(cx).busy()
            || self
                .removal
                .as_ref()
                .is_some_and(|editor| editor.read(cx).pending())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled()
            || self.info.is_none()
            || self.error.is_some()
            || self.pending(cx)
        {
            return;
        }
        match &self.target {
            Target::Installed(expected) => {
                if let Some(editor) = &self.removal {
                    editor.update(cx, |editor, cx| editor.prompt(window, cx));
                    return;
                }
                let details = cx.weak_entity();
                let editor = dialog::remove(
                    self.binding.clone(),
                    expected.clone(),
                    window,
                    cx,
                    move |window, cx| {
                        let active = details
                            .update(cx, |details, _| !details.stop.is_cancelled())
                            .unwrap_or(false);
                        if active {
                            window.close_sheet(cx);
                        }
                    },
                );
                cx.observe(&editor, |_, _, cx| cx.notify()).detach();
                self.removal = Some(editor);
                cx.notify();
            }
            Target::Catalog {
                market,
                entry,
                source,
                ..
            } => {
                let Some(market) = market.upgrade() else {
                    return;
                };
                let entry = entry.as_ref().clone();
                let source = *source;
                // Dismiss Details before acquisition opens its own dialog.
                self.stop.cancel();
                window.close_sheet(cx);
                market.update(cx, |market, cx| market.install(source, entry, window, cx));
            }
        }
    }

    fn name(&self) -> &str {
        match &self.target {
            Target::Installed(expected) => &expected.name,
            Target::Catalog { entry, .. } => &entry.name,
        }
    }

    fn version(&self) -> Option<String> {
        self.info
            .as_ref()
            .and_then(|info| info.summary.version.clone())
            .or_else(|| match &self.target {
                Target::Installed(expected) => expected.version.clone(),
                Target::Catalog { entry, .. } => entry.version.clone(),
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
            Target::Catalog { source, entry, .. } => Command::ReadCatalogPluginInfo {
                source: *source,
                id: entry.id.clone(),
                bundled: entry.bundled,
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
                            Target::Catalog { entry, .. } => info.summary.name == entry.name,
                        };
                        if matches {
                            if matches!(details.target, Target::Installed(_)) {
                                details
                                    .authorization
                                    .update(cx, |state, cx| state.accept([&info], cx));
                            }
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
