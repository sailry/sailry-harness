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
mod details;
mod install;
mod preview;
#[cfg(test)]
mod tests;
pub(super) use preview::Skill;

pub(super) struct State {
    query: Entity<InputState>,
    action: Option<action::Action>,
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
            action: None,
            error: None,
        }
    }
}

impl Workspace {
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
        let entries: Vec<_> = metadata
            .read(cx)
            .entries
            .values()
            .filter(|info| info.skill.is_some())
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
            .when_some(
                self.skill_state
                    .action
                    .as_ref()
                    .filter(|action| !action.pending),
                |view, action| {
                    view.child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(action.binding.label.clone()),
                            )
                            .child(
                                Button::new("skills-retry")
                                    .debug_selector(|| "skills-retry".into())
                                    .label(tr("plugins_retry"))
                                    .on_click(
                                        cx.listener(|owner, _, _, cx| owner.send_skill_action(cx)),
                                    ),
                            ),
                    )
                },
            )
            .when(
                entries.is_empty() && metadata.read(cx).settled() && live.connected,
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
        let busy = !live.connected || self.skill_state.action.is_some();
        let id = format!("{}:{}", info.summary.name, skill.name);
        let card_id = format!("skill-card-{id}");
        let toggle_id = format!("skill-toggle-{id}");
        let summary = info.summary.clone();
        let remove = summary.clone();
        let binding = live.binding.clone();
        let remove_binding = binding.clone();
        let owner = cx.entity();
        let title = skill.name.clone();
        let detail_info = info.clone();
        let detail_skill = skill.clone();
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
            details::open(detail_info.clone(), detail_skill.clone(), window, cx)
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
                        menu.item(
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

fn provenance(info: &Info) -> String {
    match &info.skill {
        Some(source) => source
            .source
            .repository
            .trim_end_matches(".git")
            .trim_start_matches("https://github.com/")
            .to_owned(),
        None => rust_i18n::t!("skills_package_source", name = info.summary.name).to_string(),
    }
}

fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "plugins_conflict",
        ErrorCode::NotFound => "skills_not_found",
        ErrorCode::InvalidRequest => "skills_source_invalid",
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
