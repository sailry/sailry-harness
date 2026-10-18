use super::*;

impl Market {
    fn scope(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.kind == Kind::Themes {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr("market_local"))
                .into_any_element();
        }
        if self.kind == Kind::Skills {
            let owner = self.owner.clone();
            let sources = owner
                .upgrade()
                .map(|owner| owner.read(cx).usage_sources.clone())
                .unwrap_or_default();
            return Button::new("market-install-host")
                .outline()
                .label(rust_i18n::t!("market_install_to", host = self.binding.label).to_string())
                .icon(IconName::ChevronDown)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    sources.iter().fold(menu, |menu, source| {
                        let node = source.client.target();
                        let owner = owner.clone();
                        menu.item(PopupMenuItem::new(source.label.clone()).on_click(
                            move |_, _, cx| {
                                _ = owner.update(cx, |_, cx| {
                                    cx.emit(super::super::extensions::SelectHost(node))
                                });
                            },
                        ))
                    })
                })
                .into_any_element();
        }
        let owner = cx.weak_entity();
        let source = self.source;
        Button::new("market-source")
            .outline()
            .label(tr(if source == Source::Official {
                "market_source_official"
            } else {
                "market_source_community"
            }))
            .icon(IconName::ChevronDown)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                [
                    (Source::Official, "market_source_official"),
                    (Source::ThirdParty, "market_source_community"),
                ]
                .into_iter()
                .fold(menu, |menu, (choice, key)| {
                    let owner = owner.clone();
                    menu.item(
                        PopupMenuItem::new(tr(key))
                            .checked(choice == source)
                            .on_click(move |_, _, cx| {
                                _ = owner.update(cx, |state, cx| {
                                    state.source = choice;
                                    state.page = None;
                                    state.filter.category = None;
                                    state.search(1, false, cx);
                                });
                            }),
                    )
                })
            })
            .into_any_element()
    }

    fn filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let choices: Vec<(usize, SharedString, bool)> = match self.kind {
            Kind::Plugins => {
                let mut choices = vec![(0, tr("market_all"), self.filter.category.is_none())];
                if self.source == Source::Official {
                    choices.extend(
                        [
                            Category::Development,
                            Category::Documents,
                            Category::Data,
                            Category::Automation,
                            Category::Entertainment,
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, category)| {
                            (
                                index + 1,
                                tr(match category {
                                    Category::Development => "market_category_development",
                                    Category::Documents => "market_category_documents",
                                    Category::Data => "market_category_data",
                                    Category::Automation => "market_category_automation",
                                    Category::Entertainment => "market_category_entertainment",
                                }),
                                self.filter.category == Some(category),
                            )
                        }),
                    );
                }
                choices
            }
            Kind::Skills => [
                (filter::Skills::All, "market_all"),
                (filter::Skills::Official, "market_skill_official"),
                (filter::Skills::Directory, "skills.sh"),
                (filter::Skills::Hub, "skillhub.cn"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (kind, key))| {
                (
                    index,
                    if index > 1 { key.into() } else { tr(key) },
                    self.filter.skills == kind,
                )
            })
            .collect(),
            Kind::Themes => vec![
                (0, tr("market_all"), self.filter.dark.is_none()),
                (1, tr("market_theme_dark"), self.filter.dark == Some(true)),
                (2, tr("market_theme_light"), self.filter.dark == Some(false)),
            ],
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(h_flex().flex_1().min_w_0().gap_1().flex_wrap().children(
                choices.into_iter().map(|(index, label, selected)| {
                    Button::new(("market-filter", index))
                        .ghost()
                        .small()
                        .label(label)
                        .selected(selected)
                        .on_click(cx.listener(move |state, _, _, cx| {
                            match state.kind {
                                Kind::Plugins => {
                                    state.filter.category = match index {
                                        1 => Some(Category::Development),
                                        2 => Some(Category::Documents),
                                        3 => Some(Category::Data),
                                        4 => Some(Category::Automation),
                                        5 => Some(Category::Entertainment),
                                        _ => None,
                                    }
                                }
                                Kind::Skills => {
                                    state.filter.skills = match index {
                                        1 => filter::Skills::Official,
                                        2 => filter::Skills::Directory,
                                        3 => filter::Skills::Hub,
                                        _ => filter::Skills::All,
                                    }
                                }
                                Kind::Themes => {
                                    state.filter.dark = match index {
                                        1 => Some(true),
                                        2 => Some(false),
                                        _ => None,
                                    }
                                }
                            }
                            cx.notify();
                        }))
                }),
            ))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Switch::new("market-installed")
                            .checked(self.filter.installed)
                            .accessibility_label(tr("market_installed_only"))
                            .on_click(cx.listener(|state, value, _, cx| {
                                state.filter.installed = *value;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("market_installed_only")),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Market {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .max_w(px(WIDTH))
            .mx_auto()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(div().text_xl().font_semibold().child(tr("market_title")))
                    .child(
                        h_flex()
                            .id("market-kind-group")
                            .role(Role::Group)
                            .aria_label(tr("market_kind"))
                            .child(
                                TabBar::new("market-kind")
                                    .segmented()
                                    .selected_index(self.kind as usize)
                                    .children(
                                        ["market_plugins", "market_skills", "market_themes"]
                                            .into_iter()
                                            .map(|key| Tab::new().label(tr(key))),
                                    )
                                    .on_click(cx.listener(|state, index, window, cx| {
                                        state.select_kind(
                                            match *index {
                                                1 => Kind::Skills,
                                                2 => Kind::Themes,
                                                _ => Kind::Plugins,
                                            },
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.query)
                                .cleanable(true)
                                .prefix(IconName::Search),
                        ),
                    )
                    .child(self.scope(cx)),
            )
            .child(self.filters(cx))
            .when(self.error.is_some(), |view| {
                view.child(
                    crate::feedback::retry("market-retry")
                        .on_click(cx.listener(|state, _, _, cx| state.search(1, true, cx))),
                )
            })
            .child(match self.kind {
                Kind::Plugins => self.plugin_cards(cx),
                Kind::Skills => self.skill_cards(cx),
                Kind::Themes => self.theme_cards(cx),
            })
    }
}
