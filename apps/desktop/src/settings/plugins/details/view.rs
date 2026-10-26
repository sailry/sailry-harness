use super::*;
use gpui_kit::component::{
    badge::Badge,
    tab::{Tab as KitTab, TabBar},
};
use sailry_protocol::plugin::Origin;

fn header(details: &Details, cx: &Context<Details>) -> AnyElement {
    let owner = cx.entity();
    let info = details.info.as_ref();
    let installed = matches!(details.target, Target::Installed(_));
    let managed = details.catalog_installed(cx);
    let required = details.requirement(cx);
    let catalog_update = managed
        .as_ref()
        .is_some_and(|current| details.updates.read(cx).candidate(current).is_some());
    let update = info.is_some_and(|info| {
        installed && details.updates.read(cx).candidate(&info.summary).is_some()
    });
    let disabled =
        managed.is_none() && (details.pending(cx) || details.error.is_some() || !details.ready());
    let title = info.map(metadata::title).unwrap_or_else(|| {
        details
            .external_metadata()
            .map(|metadata| metadata.name.clone())
            .unwrap_or_else(|| details.name().into())
    });
    let primary = owner.clone();
    let tabs = owner.clone();
    let extension = info.and_then(|info| info.extension.as_ref());
    v_flex()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px_7()
        .gap_3()
        .text_sm()
        .font_normal()
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(crate::plugins::emblem::grid(
                    details.name(),
                    extension.and_then(|value| value.tint),
                    info.and_then(crate::plugins::emblem::glyph),
                    info.and_then(|value| value.icon.as_deref()),
                    px(56.),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(div().text_lg().font_semibold().truncate().child(title)),
                )
                .when(
                    required.is_none()
                        && !installed
                        && ((managed.is_none() && details.installable()) || catalog_update),
                    |row| {
                        row.child(
                            Button::new("plugin-details-primary")
                                .small()
                                .disabled(
                                    disabled
                                        || (managed.is_some() && !catalog_update)
                                        || (catalog_update
                                            && (details.updates.read(cx).busy()
                                                || details.updates.read(cx).selecting())),
                                )
                                .primary()
                                .label(tr(if catalog_update {
                                    "market_update"
                                } else {
                                    "market_install"
                                }))
                                .on_click(move |_, window, cx| {
                                    primary.update(cx, |details, cx| {
                                        if catalog_update {
                                            details.update(window, cx);
                                        } else {
                                            details.submit(window, cx);
                                        }
                                    })
                                }),
                        )
                    },
                ),
        )
        .when_some(required, |view, required| {
            view.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(compatibility::message(&required, &details.binding, cx)),
                    )
                    .children(compatibility::action(
                        "plugin-details-update-sailry",
                        &details.binding,
                        cx,
                    )),
            )
        })
        .when_some(
            info.and_then(metadata::description).or_else(|| {
                details
                    .external_metadata()
                    .and_then(|metadata| metadata.description.clone())
            }),
            |view, description| {
                view.child(
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(relative(1.6))
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            },
        )
        .child(
            TabBar::new("plugin-details-tabs")
                .underline()
                .selected_index(match details.tab {
                    Tab::Overview => 0,
                    Tab::Contents => 1,
                    Tab::Permissions => 2,
                    Tab::Versions => 3,
                })
                .child(KitTab::new().label(tr("market_details_overview")))
                .child(KitTab::new().label(tr("market_details_contents")))
                .when(info.is_some(), |tabs| {
                    tabs.child(KitTab::new().label(tr("market_details_permissions")))
                })
                .when(info.is_some(), |tabs| {
                    tabs.child(KitTab::new().label(tr("market_details_versions")).when(
                        update,
                        |tab| {
                            tab.suffix(
                                Badge::new()
                                    .dot()
                                    .color(cx.theme().primary)
                                    .child(div().size_2()),
                            )
                        },
                    ))
                })
                .on_click(move |index, _, cx| {
                    tabs.update(cx, |details, cx| {
                        details.tab = match index {
                            0 => Tab::Overview,
                            1 => Tab::Contents,
                            2 => Tab::Permissions,
                            _ => Tab::Versions,
                        };
                        details.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    })
                }),
        )
        .into_any_element()
}

pub(super) fn official_repository(repository: &str) -> bool {
    url::Url::parse(repository).is_ok_and(|url| {
        let path = url.path().trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        url.host_str() == Some("github.com") && path == "/sailry/sailry-plugins"
    })
}

fn repository_owner(repository: &str) -> Option<String> {
    let url = url::Url::parse(repository).ok()?;
    if official_repository(repository) {
        return Some("Sailry".into());
    }
    url.path_segments()?
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

impl Details {
    fn update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled()
            || self.updates.read(cx).busy()
            || self.updates.read(cx).selecting()
        {
            return;
        }
        let Some(current) = self
            .catalog_installed(cx)
            .filter(|current| self.updates.read(cx).candidate(current).is_some())
        else {
            return;
        };
        self.stop.cancel();
        dismiss(window, cx);
        self.updates
            .update(cx, |state, cx| state.apply(Some(&current.name), window, cx));
    }

    fn information(&self, cx: &App) -> AnyElement {
        let source = match self.info.as_ref().and_then(|info| info.origin.as_ref()) {
            Some(Origin::Bundled) => "market_details_source_builtin",
            Some(Origin::Online { source, .. })
                if repository_owner(&source.repository).as_deref() == Some("Sailry") =>
            {
                "market_details_source_official"
            }
            Some(Origin::Online { .. } | Origin::Registry { .. }) => {
                "market_details_source_community"
            }
            _ => match &self.target {
                Target::Catalog { entry, .. } if entry.bundled => "market_details_source_builtin",
                Target::Catalog {
                    source: Source::Official,
                    ..
                } => "market_details_source_official",
                Target::Catalog {
                    source: Source::ThirdParty,
                    ..
                } => "market_details_source_community",
                Target::Installed(_) => "market_details_source_local",
            },
        };
        let mut information = vec![
            rows::row(
                "plugin-version".into(),
                IconName::Tag,
                tr("market_details_version"),
                None,
                self.version()
                    .or_else(|| {
                        self.external_metadata()
                            .and_then(|metadata| metadata.version.clone())
                    })
                    .unwrap_or_else(|| "—".into()),
                cx,
            )
            .into_any_element(),
            rows::row(
                "plugin-source".into(),
                IconName::Globe,
                tr("market_details_source"),
                None,
                tr(source),
                cx,
            )
            .into_any_element(),
            rows::row(
                "plugin-host".into(),
                IconName::Server,
                tr("market_details_host"),
                None,
                self.binding.label.clone(),
                cx,
            )
            .into_any_element(),
        ];
        if let Target::Catalog { entry, .. } = &self.target
            && let Some(repository) = &entry.repository
            && self.external.is_some()
        {
            information[1] = rows::row(
                "plugin-source".into(),
                IconName::Globe,
                tr("market_details_source"),
                None,
                repository.clone(),
                cx,
            )
            .into_any_element();
        }
        if let Some(homepage) = self
            .external_metadata()
            .and_then(|metadata| metadata.homepage.as_deref())
        {
            information.push(rows::homepage("plugin-homepage", homepage, cx));
        }
        rows::information(information, cx)
    }

    fn overview(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let mut includes = Vec::new();
        let tools = info
            .extension
            .as_ref()
            .map_or(0, |extension| extension.tools.len());
        for (key, icon, count) in [
            ("market_details_tools", IconName::Wrench, tools),
            (
                "market_details_skills",
                IconName::BookOpen,
                info.skills.len(),
            ),
            ("market_details_mcp", IconName::Server, info.mcp.len()),
        ] {
            if count == 0 {
                continue;
            }
            includes.push(
                Button::new(key)
                    .ghost()
                    .h_auto()
                    .w_full()
                    .px_0()
                    .py_0()
                    .child(rows::row(
                        key.into(),
                        icon,
                        tr(key),
                        None,
                        h_flex()
                            .gap_2()
                            .child(count.to_string())
                            .child(Icon::new(IconName::ChevronRight).size_4()),
                        cx,
                    ))
                    .on_click(cx.listener(|details, _, _, cx| {
                        details.tab = Tab::Contents;
                        details.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        v_flex()
            .gap_5()
            .child(self.information(cx))
            .when(!includes.is_empty(), |view| {
                view.child(rows::section("market_details_includes", None, includes, cx))
            })
            .into_any_element()
    }
}

impl Render for Details {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if let Some(info) = self.info.clone() {
            match self.tab {
                Tab::Overview => self.overview(&info, cx),
                Tab::Contents => self.contents(&info, cx),
                Tab::Permissions => self.permissions(&info, cx),
                Tab::Versions => self.versions(&info, cx),
            }
        } else if self.external.is_some() {
            self.external_contents(cx)
        } else if self.error.is_some() {
            crate::feedback::retry("plugin-details-reload")
                .on_click(cx.listener(|details, _, _, cx| details.load(cx)))
                .into_any_element()
        } else {
            crate::settings::market::details::skeleton(cx).into_any_element()
        };
        crate::settings::market::details::panel(
            "market_plugin_details",
            header(self, cx),
            content,
            &self.scroll,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn identifies_repository_publishers() {
        assert_eq!(
            repository_owner("https://github.com/sailry/sailry-plugins/"),
            Some("Sailry".into())
        );
        assert_eq!(
            repository_owner("https://github.com/sailry/sailry-plugins.git"),
            Some("Sailry".into())
        );
        assert_eq!(
            repository_owner("https://github.com/community/tools"),
            Some("community".into())
        );
        assert_eq!(
            repository_owner("https://example.org/sailry/sailry-plugins"),
            Some("sailry".into())
        );
        assert_eq!(repository_owner("not a repository"), None);
    }
}
