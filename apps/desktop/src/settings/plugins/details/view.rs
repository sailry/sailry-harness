use super::*;
use gpui_kit::component::{
    badge::Badge,
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    skeleton::Skeleton,
    tab::{Tab as KitTab, TabBar},
};
use sailry_protocol::plugin::Origin;

fn header(details: &Details, cx: &Context<Details>) -> AnyElement {
    let owner = cx.entity();
    let info = details.info.as_ref();
    let installed = matches!(details.target, Target::Installed(_));
    let update = info.is_some_and(|info| {
        installed && details.updates.read(cx).candidate(&info.summary).is_some()
    });
    let configurable =
        installed && info.is_some_and(super::super::super::extensions::modal::configurable);
    let disabled = details.pending(cx) || details.error.is_some() || !details.ready();
    let title = info.map(metadata::title).unwrap_or_else(|| {
        details
            .external_metadata()
            .map(|metadata| metadata.name.clone())
            .unwrap_or_else(|| details.name().into())
    });
    let publisher = info
        .map(publisher)
        .or_else(|| {
            details
                .external_metadata()
                .and_then(|metadata| metadata.author.clone())
        })
        .unwrap_or_default();
    let subtitle = byline(&publisher, details.version().as_deref(), installed);
    let primary = owner.clone();
    let more = owner.clone();
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
                        .child(div().text_lg().font_semibold().truncate().child(title))
                        .child(
                            div()
                                .text_xs()
                                .font_normal()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(subtitle),
                        ),
                )
                .when(
                    (!installed && details.installable()) || update || configurable,
                    |row| {
                        row.child(
                            Button::new("plugin-details-primary")
                                .small()
                                .disabled(disabled)
                                .when(update || !installed, |button| button.primary())
                                .label(tr(if update {
                                    "market_update"
                                } else if installed {
                                    "plugins_configure"
                                } else {
                                    "market_install"
                                }))
                                .on_click(move |_, window, cx| {
                                    primary.update(cx, |details, cx| {
                                        if update {
                                            details.update(window, cx);
                                        } else if installed {
                                            details.configure(window, cx);
                                        } else {
                                            details.submit(window, cx);
                                        }
                                    })
                                }),
                        )
                    },
                )
                .when(installed, |row| {
                    row.child(
                        Button::new("plugin-details-more")
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .accessibility_label(tr("market_details_more"))
                            .disabled(disabled)
                            .dropdown_menu(move |menu, _, _| {
                                let more = more.clone();
                                menu.item(
                                    PopupMenuItem::new(tr("market_details_uninstall")).on_click(
                                        move |_, window, cx| {
                                            more.update(cx, |details, cx| {
                                                details.submit(window, cx)
                                            })
                                        },
                                    ),
                                )
                            }),
                    )
                }),
        )
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

fn publisher(info: &Info) -> String {
    match info.origin.as_ref() {
        Some(Origin::Bundled) => "Sailry".into(),
        Some(Origin::Online { source, .. }) => repository_owner(&source.repository)
            .unwrap_or_else(|| tr("market_details_local").to_string()),
        Some(Origin::Registry { source }) => source.namespace.clone(),
        _ => tr("market_details_local").to_string(),
    }
}

fn repository_owner(repository: &str) -> Option<String> {
    let url = url::Url::parse(repository).ok()?;
    if url.host_str() == Some("github.com")
        && url.path().trim_end_matches('/') == "/sailry/sailry-plugins"
    {
        return Some("Sailry".into());
    }
    url.path_segments()?
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

impl Details {
    fn update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() || self.pending(cx) || self.error.is_some() {
            return;
        }
        let Target::Installed(expected) = &self.target else {
            return;
        };
        if self.updates.read(cx).candidate(expected).is_none() {
            return;
        }
        let name = expected.name.clone();
        self.stop.cancel();
        super::dismiss(window, cx);
        self.updates
            .update(cx, |state, cx| state.apply(Some(&name), window, cx));
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
        let source = match info.origin.as_ref() {
            Some(Origin::Bundled) => "market_details_source_builtin",
            Some(Origin::Online { source, .. })
                if repository_owner(&source.repository).as_deref() == Some("Sailry") =>
            {
                "market_details_source_official"
            }
            Some(Origin::Online { .. } | Origin::Registry { .. }) => {
                "market_details_source_community"
            }
            _ => "market_details_source_local",
        };
        let information = vec![
            rows::row(
                "plugin-version".into(),
                IconName::Tag,
                tr("market_details_version"),
                None,
                self.version().unwrap_or_else(|| "—".into()),
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
        v_flex()
            .gap_5()
            .when(!includes.is_empty(), |view| {
                view.child(rows::section("market_details_includes", None, includes, cx))
            })
            .child(rows::section(
                "market_details_information",
                None,
                information,
                cx,
            ))
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
            v_flex()
                .w_full()
                .gap_3()
                .children((0..3).map(|_| Skeleton::new().h_10().w_full()))
                .into_any_element()
        };
        v_flex()
            .id("plugin-details")
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .text_sm()
            .font_normal()
            .child(
                h_flex()
                    .w_full()
                    .h(px(52.))
                    .flex_shrink_0()
                    .px_3()
                    .justify_end()
                    .child(
                        Button::new("plugin-details-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .accessibility_label(tr("close"))
                            .on_click(|_, window, cx| super::dismiss(window, cx)),
                    ),
            )
            .child(header(self, cx))
            .child(
                div()
                    .id("plugin-details-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(div().w_full().min_w_0().px_7().pt_5().pb_7().child(content))
                    .vertical_scrollbar(&self.scroll),
            )
            .into_any_element()
    }
}

fn byline(publisher: &str, version: Option<&str>, installed: bool) -> String {
    let Some(version) = version else {
        return publisher.into();
    };
    let version = if installed {
        format!("{} {version}", tr("market_installed"))
    } else {
        version.into()
    };
    if publisher.is_empty() {
        version
    } else {
        format!("{publisher} · {version}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn formats_version_once() {
        assert_eq!(byline("Sailry", None, true), "Sailry");
        assert_eq!(byline("Sailry", Some("0.1.7"), false), "Sailry · 0.1.7");
        assert_eq!(
            byline("Sailry", Some("0.1.7"), true),
            format!("Sailry · {} 0.1.7", tr("market_installed")),
        );
        assert_eq!(byline("", Some("0.1.7"), false), "0.1.7");
    }

    #[test]
    fn identifies_repository_publishers() {
        assert_eq!(
            repository_owner("https://github.com/sailry/sailry-plugins/"),
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
