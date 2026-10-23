use super::{Workspace, providers::Binding};
use crate::settings::resource_card;
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, plugin::Info};

mod action;
pub(in crate::settings) mod install;
mod preview;
pub(super) use preview::Skill;

pub(super) struct State {
    query: Entity<InputState>,
    actions: std::collections::BTreeMap<(sailry_protocol::NodeId, String), action::Action>,
    pub(super) error: Option<&'static str>,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("skills_search")));
        cx.subscribe(&query, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            query,
            actions: Default::default(),
            error: None,
        }
    }
}

pub(in crate::settings) fn open_management(
    owner: Entity<Workspace>,
    binding: Binding,
    info: Info,
    name: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let selected = name
        .and_then(|name| info.skills.iter().find(|skill| skill.name == name))
        .or_else(|| info.skills.first())
        .cloned();
    if let Some(skill) = selected {
        super::market::details::skills::open_installed(owner, binding, info, skill, window, cx);
    }
}

impl Workspace {
    pub(in crate::settings) fn reimport_skill(
        &mut self,
        summary: sailry_protocol::plugin::Summary,
        kind: sailry_protocol::plugin::UploadSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = self.provider_link.as_ref().filter(|live| live.connected) else {
            return;
        };
        install::reimport(
            live.binding.clone(),
            self.plugin_catalog.packages.clone(),
            summary,
            kind,
            window,
            cx,
        );
    }

    pub(super) fn skills(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(live) = &self.provider_link else {
            return self.preview_skills(cx);
        };
        let query = self
            .skill_state
            .query
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let metadata = self
            .plugin_catalog
            .metadata
            .as_ref()
            .expect("skills metadata is bound");
        let entries: Vec<_> = super::plugins::classification::inventory(
            &self.plugin_catalog.packages,
            &metadata.read(cx).entries,
            super::plugins::classification::Kind::Skill,
        )
        .into_iter()
        .flat_map(|info| {
            info.skills.iter().filter_map(|skill| {
                let source = provenance(info);
                (query.is_empty()
                    || format!("{} {} {source}", skill.name, skill.description)
                        .to_lowercase()
                        .contains(&query))
                .then_some((info.clone(), skill.clone()))
            })
        })
        .collect();
        let binding = live.binding.clone();
        let known = self.plugin_catalog.packages.clone();
        let checking = self
            .plugin_catalog
            .updates
            .as_ref()
            .is_some_and(|state| state.read(cx).checking());
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(|| "skills-search".into())
                            .child(
                                Input::new(&self.skill_state.query)
                                    .cleanable(true)
                                    .prefix(Icon::new(IconName::Search).small()),
                            ),
                    )
                    .child(
                        Button::new("skills-check-updates")
                            .label(tr("plugins_check_updates"))
                            .loading(checking)
                            .disabled(
                                !live.connected
                                    || self.plugin_catalog.updates.as_ref().is_some_and(|state| {
                                        state.read(cx).busy() || state.read(cx).selecting()
                                    }),
                            )
                            .on_click(cx.listener(|owner, _, window, cx| {
                                owner.check_plugin_updates(window, cx)
                            })),
                    )
                    .child(
                        Button::new("skills-install")
                            .debug_selector(|| "skills-install".into())
                            .primary()
                            .label(tr("skills_install"))
                            .disabled(!live.connected)
                            .on_click(move |_, window, cx| {
                                install::open(binding.clone(), known.clone(), window, cx)
                            }),
                    ),
            )
            .children(
                self.skill_state
                    .actions
                    .iter()
                    .filter(|(key, action)| {
                        key.0 == live.binding.client.target() && !action.pending
                    })
                    .map(|(key, action)| {
                        let key = key.clone();
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} · {}", action.binding.label, key.1)),
                            )
                            .child(
                                crate::feedback::retry(SharedString::from(format!(
                                    "skills-retry-{}",
                                    key.1
                                )))
                                .on_click(cx.listener(
                                    move |owner, _, _, cx| {
                                        if owner.provider_link.as_ref().is_some_and(|live| {
                                            live.binding.client.target() == key.0
                                        }) {
                                            owner.send_skill_action(&key, cx);
                                        }
                                    },
                                )),
                            )
                    }),
            )
            .when(!metadata.read(cx).errors.is_empty(), |view| {
                view.child(
                    crate::feedback::retry("skills-metadata-retry").on_click(cx.listener(
                        |owner, _, _, cx| {
                            if let Some(metadata) = &owner.plugin_catalog.metadata {
                                metadata.update(cx, |metadata, cx| metadata.refresh(cx));
                            }
                        },
                    )),
                )
            })
            .when(
                entries.is_empty()
                    && metadata.read(cx).settled()
                    && metadata.read(cx).errors.is_empty()
                    && live.connected
                    && live.snapshot.is_some(),
                |view| {
                    view.child(
                        crate::settings::group::Group::new("skills_installed")
                            .heading(false)
                            .empty(
                                IconName::Inbox,
                                if query.is_empty() {
                                    "plugins_no_skills"
                                } else {
                                    "skills_no_matches"
                                },
                            ),
                    )
                },
            )
            .child(
                resource_card::grid()
                    .debug_selector(|| "skill-cards".into())
                    .children(
                        entries
                            .into_iter()
                            .map(|(info, skill)| self.skill_card(&info, &skill, cx)),
                    ),
            )
            .into_any_element()
    }

    fn skill_card(
        &self,
        info: &Info,
        skill: &sailry_protocol::plugin::Skill,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.provider_link.as_ref().unwrap();
        let busy = !live.connected
            || self
                .skill_state
                .actions
                .contains_key(&(live.binding.client.target(), info.summary.name.clone()))
            || self.plugin_catalog.busy_package(&info.summary.name, cx);
        let node = live.binding.client.target();
        let id = format!("{}:{}", info.summary.name, skill.name);
        let card_id = format!("skill-card-{id}");
        let toggle_id = format!("skill-toggle-{id}");
        let summary = info.summary.clone();
        let remove = summary.clone();
        let binding = live.binding.clone();
        let remove_binding = binding.clone();
        let owner = cx.entity();
        let details_owner = owner.clone();
        let details_binding = binding.clone();
        let title = skill.name.clone();
        let detail_info = info.clone();
        let detail_skill = skill.clone();
        let menu_info = info.clone();
        let menu_skill = skill.clone();
        let imported = match &info.skill {
            Some(sailry_protocol::plugin::skills::Provenance::Imported { imported }) => {
                Some(imported.kind)
            }
            _ => None,
        };
        let reimport = summary.clone();
        let update = summary.name.clone();
        let available = self
            .plugin_catalog
            .updates
            .as_ref()
            .is_some_and(|state| state.read(cx).has_candidate(&summary.name));
        let update_busy = self
            .plugin_catalog
            .updates
            .as_ref()
            .is_some_and(|state| state.read(cx).busy() || state.read(cx).selecting());
        let icon = sailry_protocol::plugin::desktop::Icon::Name("reicon:school/book".into());
        let content = resource_card::summary(
            format!("skill-details-{id}"),
            skill
                .display_name
                .clone()
                .unwrap_or_else(|| skill.name.clone()),
            skill.description.clone(),
            crate::plugins::emblem::render(&info.summary.name, Some(&icon), None, px(48.), cx),
            cx,
        )
        .on_click(move |_, window, cx| {
            if details_owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_none_or(|live| live.binding.client.target() != node)
            {
                return;
            }
            super::market::details::skills::open_installed(
                details_owner.clone(),
                details_binding.clone(),
                detail_info.clone(),
                detail_skill.clone(),
                window,
                cx,
            )
        });
        let actions = h_flex()
            .items_center()
            .gap_2()
            .child(
                resource_card::menu(format!("skill-menu-{id}"), card_id.clone().into())
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let owner = owner.clone();
                        let binding = remove_binding.clone();
                        let remove = remove.clone();
                        let title = title.clone();
                        let target = reimport.clone();
                        let reimport_owner = owner.clone();
                        let update_owner = owner.clone();
                        let update = update.clone();
                        let details_owner = owner.clone();
                        let details_binding = binding.clone();
                        let info = menu_info.clone();
                        let skill = menu_skill.clone();
                        menu.item(
                            PopupMenuItem::new(tr("plugins_information"))
                                .icon(gpui_kit::assets::IconName::Info)
                                .on_click(move |_, window, cx| {
                                    if details_owner
                                        .read(cx)
                                        .provider_link
                                        .as_ref()
                                        .is_none_or(|live| live.binding.client.target() != node)
                                    {
                                        return;
                                    }
                                    super::market::details::skills::open_installed(
                                        details_owner.clone(),
                                        details_binding.clone(),
                                        info.clone(),
                                        skill.clone(),
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .when(available, |menu| {
                            menu.item(
                                PopupMenuItem::new(tr("settings_update"))
                                    .icon(IconName::Redo)
                                    .disabled(busy || update_busy)
                                    .on_click(move |_, window, cx| {
                                        update_owner.update(cx, |owner, cx| {
                                            if owner.provider_link.as_ref().is_some_and(|live| {
                                                live.binding.client.target() == node
                                            }) {
                                                owner.update_plugins(Some(&update), window, cx);
                                            }
                                        });
                                    }),
                            )
                        })
                        .when_some(imported, |menu, kind| {
                            menu.item(
                                PopupMenuItem::new(tr("skill_source_reimport"))
                                    .icon(IconName::Redo)
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        reimport_owner.update(cx, |owner, cx| {
                                            if owner.provider_link.as_ref().is_some_and(|live| {
                                                live.binding.client.target() == node
                                            }) {
                                                owner.reimport_skill(
                                                    target.clone(),
                                                    kind,
                                                    window,
                                                    cx,
                                                );
                                            }
                                        });
                                    }),
                            )
                        })
                        .item(
                            PopupMenuItem::new(tr("plugins_uninstall"))
                                .icon(IconName::CircleX)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    let owner = owner.clone();
                                    let binding = binding.clone();
                                    let remove = remove.clone();
                                    crate::prompts::confirm(
                                        &tr("plugins_uninstall"),
                                        &rust_i18n::t!("skills_remove_effect", name = title),
                                        tr("plugins_uninstall"),
                                        window,
                                        cx,
                                        move |_, cx| {
                                            owner.update(cx, |owner, cx| {
                                                if owner.provider_link.as_ref().is_none_or(|live| {
                                                    live.binding.client.target() != node
                                                }) {
                                                    return;
                                                }
                                                owner.skill_action(
                                                    binding.clone(),
                                                    Command::RemovePlugin {
                                                        name: remove.name.clone(),
                                                        expected_revision: remove.revision,
                                                    },
                                                    cx,
                                                )
                                            })
                                        },
                                    );
                                }),
                        )
                    }),
            )
            .child(
                div().debug_selector(move || toggle_id.clone()).child(
                    Switch::new(SharedString::from(format!("skill-toggle-{id}")))
                        .checked(info.summary.enabled)
                        .disabled(busy)
                        .accessibility_label(tr("plugins_enabled"))
                        .on_click(cx.listener(move |owner, enabled, _, cx| {
                            if owner
                                .provider_link
                                .as_ref()
                                .is_none_or(|live| live.binding.client.target() != node)
                            {
                                return;
                            }
                            owner.skill_action(
                                binding.clone(),
                                Command::SetPluginEnabled {
                                    name: summary.name.clone(),
                                    expected_revision: summary.revision,
                                    enabled: *enabled,
                                },
                                cx,
                            );
                        })),
                ),
            );
        resource_card::card(card_id, content, actions, cx)
    }
}

pub(in crate::settings) fn provenance(info: &Info) -> String {
    match &info.skill {
        Some(sailry_protocol::plugin::skills::Provenance::Git(source)) => source
            .source
            .repository
            .trim_end_matches(".git")
            .trim_start_matches("https://github.com/")
            .to_owned(),
        Some(sailry_protocol::plugin::skills::Provenance::Registry { registry }) => {
            format!(
                "skillhub.cn/skills/{}/{}",
                registry.namespace, registry.slug
            )
        }
        Some(sailry_protocol::plugin::skills::Provenance::Imported { imported }) => {
            tr(match imported.kind {
                sailry_protocol::plugin::UploadSource::Directory => "skill_source_directory",
                sailry_protocol::plugin::UploadSource::Archive => "skill_source_archive",
            })
            .to_string()
        }
        None => rust_i18n::t!("skills_package_source", name = info.summary.name).to_string(),
    }
}

fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "plugins_conflict",
        ErrorCode::NotFound => "skills_not_found",
        ErrorCode::InvalidRequest => "skill_source_invalid",
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => "plugins_unknown",
        ErrorCode::Busy => "plugins_busy",
        _ => "skills_failed",
    }
}

fn uncertain(error: &Fault) -> bool {
    matches!(
        error.code,
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
    )
}
