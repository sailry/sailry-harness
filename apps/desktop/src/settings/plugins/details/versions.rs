use super::*;
use gpui_kit::component::{accordion::Accordion, tag::Tag};
use sailry_protocol::plugin::catalog::ReleaseNotes;

fn current_notes<'a>(info: &'a Info, target: &'a Target) -> Option<&'a ReleaseNotes> {
    info.release_notes.as_ref().or(match target {
        Target::Catalog { entry, .. } => entry.release_notes.as_ref(),
        Target::Installed(_) => None,
    })
}

impl Details {
    pub(super) fn versions(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let installed = matches!(self.target, Target::Installed(_));
        let candidate = installed
            .then(|| self.updates.read(cx).candidate(&info.summary))
            .flatten();
        let mut rows = Vec::new();
        if let Some(candidate) = candidate {
            rows.push(
                super::rows::row(
                    "plugin-latest-version".into(),
                    IconName::Tag,
                    candidate
                        .summary
                        .version
                        .clone()
                        .unwrap_or_else(|| "—".into()),
                    None,
                    Tag::primary().small().child(tr("market_details_latest")),
                    cx,
                )
                .into_any_element(),
            );
            if let Some(notes) = candidate.release_notes.as_ref() {
                rows.push(notes::view(notes));
            }
            let added = self.updates.read(cx).added_actions(info);
            let labels: Vec<_> = candidate
                .extension
                .iter()
                .flat_map(|extension| &extension.actions)
                .filter(|action| added.contains(&permissions::raw(action)))
                .map(|action| tr(permissions::label(action)).to_string())
                .collect();
            if !labels.is_empty() {
                rows.push(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .items_start()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(Icon::new(IconName::Shield).size_4().flex_shrink_0())
                        .child(format!(
                            "{}: {}",
                            tr("market_details_added_permissions"),
                            labels.join(", ")
                        ))
                        .into_any_element(),
                );
            }
        }
        if let Some(blocked) = installed
            .then(|| self.updates.read(cx).blocked(&info.summary))
            .flatten()
        {
            rows.push(
                super::rows::row(
                    "plugin-blocked-version".into(),
                    IconName::Tag,
                    blocked.version.clone(),
                    Some(compatibility::message(&blocked.required, &self.binding, cx).to_string()),
                    h_flex().children(compatibility::action(
                        "plugin-version-update-sailry",
                        &self.binding,
                        cx,
                    )),
                    cx,
                )
                .into_any_element(),
            );
            if let Some(notes) = blocked.release_notes.as_ref() {
                rows.push(notes::view(notes));
            }
        }
        rows.push(
            super::rows::row(
                "plugin-installed-version".into(),
                IconName::Tag,
                self.version().unwrap_or_else(|| "—".into()),
                None,
                div().when(installed, |view| {
                    view.child(super::rows::tag(tr("market_installed")))
                }),
                cx,
            )
            .into_any_element(),
        );
        if let Some(notes) = current_notes(info, &self.target) {
            rows.push(notes::view(notes));
        }
        let history = if installed {
            &self.history.versions
        } else {
            &info.versions
        };
        if !history.is_empty() {
            let owner = cx.entity();
            rows.push(
                Accordion::new("plugin-version-history")
                    .small()
                    .w_full()
                    .h_auto()
                    .bordered(false)
                    .item(|item| {
                        item.title(tr("plugins_version_history"))
                            .open(self.history.open)
                            .child(v_flex().w_full().children(history.iter().map(|version| {
                                v_flex()
                                    .w_full()
                                    .child(super::rows::row(
                                        format!("plugin-history-{}", version.version),
                                        IconName::Tag,
                                        version.version.clone(),
                                        version.compatibility.required().map(|required| {
                                            compatibility::message(required, &self.binding, cx)
                                                .to_string()
                                        }),
                                        div().when(version.compatibility.is_compatible(), |view| {
                                            view.child(super::rows::tag(tr(
                                                "plugins_version_compatible",
                                            )))
                                        }),
                                        cx,
                                    ))
                                    .when_some(version.release_notes.as_ref(), |view, notes| {
                                        view.child(notes::view(notes))
                                    })
                            })))
                    })
                    .on_toggle_click(move |open, _, cx| {
                        owner.update(cx, |details, cx| {
                            details.history.open = open.contains(&0);
                            cx.notify();
                        })
                    })
                    .into_any_element(),
            );
        } else if self.history.pending {
            rows.push(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .child(spinner::Spinner::new().small())
                    .child(tr("plugins_version_history"))
                    .into_any_element(),
            );
        }
        super::rows::section("market_details_versions", None, rows, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn keeps_installed_notes_with_their_version() {
        let info: Info = serde_json::from_value(serde_json::json!({
            "summary": {"name":"example", "revision":1, "digest":"capture", "settings_revision":0,
                "version":"0.1.0"},
            "release_notes": {"text":"Installed notes", "locales":{"zh-CN":"已安装版本说明"}},
            "versions": [{"version":"0.2.0", "commit":"a".repeat(40), "release_notes":{"text":"Newer notes"}}],
        })).unwrap();
        let target = Target::Installed(info.summary.clone());
        let notes = current_notes(&info, &target).unwrap();
        assert_eq!(notes.text("zh-CN"), "已安装版本说明");
        assert_eq!(notes.text("ja"), "Installed notes");
    }
}
