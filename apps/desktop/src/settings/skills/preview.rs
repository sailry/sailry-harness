use super::super::entry::Entry;
use super::super::{Workspace, draft, group::Group};
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    switch::Switch,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(Clone)]
pub(in crate::settings) struct Skill {
    pub namespace: String,
    pub name: String,
    pub url: String,
    pub git_ref: String,
    pub subdirectory: String,
    pub enabled: bool,
    pub managed: bool,
}

impl Skill {
    pub fn id(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    pub fn examples() -> Vec<Self> {
        vec![Self {
            namespace: "product".into(),
            name: "preview-review".into(),
            url: String::new(),
            git_ref: String::new(),
            subdirectory: String::new(),
            enabled: true,
            managed: false,
        }]
    }
}

impl Workspace {
    pub(super) fn preview_skills(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity();
        let mut group = Group::new("skills_installed");
        for (index, skill) in self.skills.iter().enumerate() {
            let edit = cx.entity();
            let remove = cx.entity();
            let id = skill.id();
            group = group.child(
                Entry::new(
                    format!("skill-{index}"),
                    v_flex()
                        .gap_1()
                        .child(div().truncate().child(id.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(if skill.managed {
                                    format!("{} / {}", skill.url, skill.git_ref)
                                } else {
                                    tr("skills_product_source").to_string()
                                }),
                        ),
                )
                .when(skill.managed, |row| {
                    row.control(
                        Switch::new(("skill-enabled", index))
                            .checked(skill.enabled)
                            .accessibility_label(id)
                            .on_click(cx.listener(move |this, value, _, cx| {
                                this.skills[index].enabled = *value;
                                cx.notify();
                            })),
                    )
                    .action(
                        Button::new(("skill-update", index))
                            .icon(IconName::Redo)
                            .tooltip(tr("settings_update"))
                            .accessibility_label(tr("settings_update"))
                            .on_click(move |_, window, cx| {
                                edit_skill(edit.clone(), Some(index), window, cx)
                            }),
                    )
                    .action(
                        Button::new(("skill-remove", index))
                            .icon(IconName::CircleX)
                            .tooltip(tr("settings_delete"))
                            .accessibility_label(tr("settings_delete"))
                            .on_click(move |_, window, cx| {
                                let owner = remove.clone();
                                draft::confirm(
                                    "settings_delete",
                                    move |cx| {
                                        owner.update(cx, |this, cx| {
                                            this.skills.remove(index);
                                            cx.notify();
                                        })
                                    },
                                    window,
                                    cx,
                                );
                            }),
                    )
                }),
            );
        }
        group
            .action(
                Button::new("skills-discover")
                    .debug_selector(|| "skills-discover".into())
                    .label(tr("skills_discover"))
                    .on_click(|_, window, cx| {
                        crate::feedback::info(
                            &tr("skills_discover"),
                            &tr("skills_discover_preview"),
                            window,
                            cx,
                        );
                    }),
            )
            .action(
                Button::new("skills-install")
                    .primary()
                    .debug_selector(|| "skills-install".into())
                    .label(tr("skills_install"))
                    .on_click(move |_, window, cx| edit_skill(owner.clone(), None, window, cx)),
            )
            .into_any_element()
    }
}

fn edit_skill(owner: Entity<Workspace>, index: Option<usize>, window: &mut Window, cx: &mut App) {
    let initial = index.map(|index| owner.read(cx).skills[index].clone());
    let mut fields = vec![
        (
            "skills_url",
            initial.as_ref().map(|s| s.url.clone()).unwrap_or_default(),
        ),
        (
            "skills_ref",
            initial
                .as_ref()
                .map(|s| s.git_ref.clone())
                .unwrap_or("HEAD".into()),
        ),
        (
            "skills_subdirectory",
            initial
                .as_ref()
                .map(|s| s.subdirectory.clone())
                .unwrap_or_default(),
        ),
    ];
    if index.is_none() {
        fields.extend([
            ("skills_namespace", "user".into()),
            ("settings_name", String::new()),
        ]);
    }
    draft::open(
        if index.is_some() {
            "skills_update"
        } else {
            "skills_install"
        },
        fields,
        move |values, _, cx| {
            let namespace = initial
                .as_ref()
                .map(|s| s.namespace.clone())
                .unwrap_or_else(|| values[3].trim().to_owned());
            let name = initial
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| values[4].trim().to_owned());
            if !values[0].starts_with("https://")
                || !(values[1] == "HEAD"
                    || values[1].starts_with("refs/heads/")
                    || values[1].starts_with("refs/tags/"))
                || namespace.is_empty()
                || name.is_empty()
            {
                return Err("skills_invalid");
            }
            owner.update(cx, |this, cx| {
                if this.skills.iter().enumerate().any(|(i, skill)| {
                    Some(i) != index && skill.namespace == namespace && skill.name == name
                }) {
                    return Err("skills_duplicate");
                }
                let skill = Skill {
                    namespace,
                    name,
                    url: values[0].clone(),
                    git_ref: values[1].clone(),
                    subdirectory: values[2].clone(),
                    managed: true,
                    enabled: initial.as_ref().is_none_or(|s| s.enabled),
                };
                if let Some(index) = index {
                    this.skills[index] = skill;
                } else {
                    this.skills.push(skill);
                }
                cx.notify();
                Ok(())
            })
        },
        window,
        cx,
    );
}
