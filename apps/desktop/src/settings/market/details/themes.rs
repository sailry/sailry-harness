use super::*;
use crate::{
    settings::appearance::updates,
    theme::{self, market as service},
};
use gpui_kit::component::{
    accordion::Accordion,
    group_box::{GroupBox, GroupBoxVariants},
};

pub(in crate::settings::market) struct Details {
    market: WeakEntity<Market>,
    id: String,
    package: Option<theme::Package>,
    source: Option<service::Selection>,
    updates: Entity<updates::State>,
    stop: CancellationToken,
    loading: bool,
    error: Option<&'static str>,
    license_open: bool,
    scroll: ScrollHandle,
    task: Option<Task<()>>,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(in crate::settings::market) fn open(
    market: Entity<Market>,
    package: Option<theme::Package>,
    source: Option<service::Selection>,
    updates: Entity<updates::State>,
    stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<Details>> {
    let Some(id) = package
        .as_ref()
        .map(|package| package.id.clone())
        .or_else(|| source.as_ref().map(|source| source.entry.id.clone()))
    else {
        crate::feedback::error("", &tr("appearance_unavailable"), window, cx);
        return None;
    };
    let node = market.read(cx).binding.client.target();
    let workspace = market.read(cx).owner.upgrade();
    let details = cx.new(|cx| {
        crate::feedback::observe(window, cx, |view: &Details, _| {
            view.error.into_iter().collect()
        });
        cx.observe_in(
            &market,
            window,
            move |view: &mut Details, market, window, cx| {
                let market = market.read(cx);
                if view.stop.is_cancelled()
                    || market.kind != Kind::Themes
                    || market.binding.client.target() != node
                {
                    window.dispatch_action(
                        Box::new(crate::settings::CloseOwnedDetails(cx.entity_id())),
                        cx,
                    );
                } else {
                    cx.notify();
                }
            },
        )
        .detach();
        if let Some(workspace) = &workspace {
            let section = workspace.read(cx).section;
            cx.observe_in(
                workspace,
                window,
                move |_: &mut Details, workspace, window, cx| {
                    let workspace = workspace.read(cx);
                    if workspace.section != section
                        || workspace
                            .provider_link
                            .as_ref()
                            .is_none_or(|live| live.binding.client.target() != node)
                    {
                        window.dispatch_action(
                            Box::new(crate::settings::CloseOwnedDetails(cx.entity_id())),
                            cx,
                        );
                    }
                },
            )
            .detach();
        }
        cx.observe(&updates, |_: &mut Details, _, cx| cx.notify())
            .detach();
        Details {
            market: market.downgrade(),
            id,
            package,
            source,
            updates,
            stop,
            loading: false,
            error: None,
            license_open: false,
            scroll: ScrollHandle::new(),
            task: None,
        }
    });
    details.update(cx, |view, cx| view.load(cx));
    window.dispatch_action(
        Box::new(crate::settings::OpenDetails(details.clone().into())),
        cx,
    );
    Some(details)
}

impl Details {
    fn installed(&self, cx: &App) -> Option<theme::Package> {
        cx.global::<theme::Catalog>()
            .packages
            .iter()
            .find(|package| package.id == self.id)
            .cloned()
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.package.is_some() || self.loading || self.stop.is_cancelled() {
            return;
        }
        let (Some(source), Some(service)) =
            (self.source.clone(), self.updates.read(cx).service.clone())
        else {
            self.error = Some("appearance_load_failed");
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let stop = self.stop.clone();
        let cancelled = stop.clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(async move { service.download(source, &cancelled).await });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = job.await;
            _ = view.update(cx, |view, cx| {
                if stop.is_cancelled() {
                    return;
                }
                view.loading = false;
                view.task = None;
                match result {
                    Ok(Ok(prepared)) => {
                        view.package = Some(theme::Package::preview(&prepared.loaded));
                    }
                    _ => view.error = Some("appearance_load_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn install(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() || self.package.is_none() {
            return;
        }
        let (Some(source), Some(market)) = (&self.source, self.market.upgrade()) else {
            return;
        };
        market.update(cx, |market, cx| {
            market.install_theme(source.clone(), window, cx);
        });
    }

    fn content(&self, package: &theme::Package, cx: &mut Context<Self>) -> AnyElement {
        let mut body =
            v_flex()
                .w_full()
                .min_w_0()
                .gap_5()
                .child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .children([false, true].map(|dark| {
                            GroupBox::new()
                                .id(if dark {
                                    "market-theme-dark"
                                } else {
                                    "market-theme-light"
                                })
                                .outline()
                                .min_w_0()
                                .content_style(
                                    StyleRefinement::default().p_0().gap_0().overflow_hidden(),
                                )
                                .child(theme::preview(package, dark, cx))
                                .child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr(if dark {
                                            "appearance_dark"
                                        } else {
                                            "appearance_light"
                                        })),
                                )
                        })),
                );
        if let Some(manifest) = &package.manifest {
            let information = vec![
                rows::row(
                    "market-theme-author".into(),
                    IconName::Users,
                    tr("appearance_author"),
                    None,
                    manifest.license.author.clone(),
                    cx,
                )
                .into_any_element(),
                rows::row(
                    "market-theme-version".into(),
                    IconName::Tag,
                    tr("settings_version"),
                    None,
                    manifest
                        .package_version
                        .clone()
                        .unwrap_or_else(|| manifest.version.to_string()),
                    cx,
                )
                .into_any_element(),
                rows::row(
                    "market-theme-id".into(),
                    IconName::Code,
                    tr("appearance_identifier"),
                    None,
                    manifest.id.clone(),
                    cx,
                )
                .into_any_element(),
            ];
            body = body.child(rows::information(information, cx)).child(
                Accordion::new("market-theme-license")
                    .w_full()
                    .h_auto()
                    .item(|item| {
                        item.open(self.license_open)
                            .title(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_3()
                                    .child(tr("settings_license"))
                                    .when_some(manifest.license.name.clone(), |title, name| {
                                        title.child(
                                            div()
                                                .ml_auto()
                                                .min_w_0()
                                                .text_color(cx.theme().muted_foreground)
                                                .whitespace_normal()
                                                .child(name),
                                        )
                                    }),
                            )
                            .content_style(StyleRefinement::default().px_4().pb_4())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .child(manifest.license.text.clone()),
                            )
                    })
                    .on_toggle_click(cx.listener(|view, open: &[usize], _, cx| {
                        view.license_open = open.contains(&0);
                        cx.notify();
                    })),
            );
        }
        body.into_any_element()
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let installed = self.installed(cx);
        let package = installed.as_ref().or(self.package.as_ref()).cloned();
        let title = package
            .as_ref()
            .map(|package| package.name.clone())
            .or_else(|| {
                self.source
                    .as_ref()
                    .map(|source| source.entry.name.clone().into())
            })
            .unwrap_or_default();
        let description = package
            .as_ref()
            .and_then(|package| package.manifest.as_ref())
            .and_then(|manifest| manifest.description.clone())
            .or_else(|| {
                self.source.as_ref().and_then(|source| {
                    source
                        .entry
                        .description(&rust_i18n::locale())
                        .map(str::to_owned)
                })
            });
        let pending = self.updates.read(cx).pending(&self.id);
        let action = if installed.is_some() {
            let id = self.id.clone();
            let updates = self.updates.clone();
            let update_id = self.id.clone();
            Some(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("market-theme-apply")
                            .small()
                            .label(tr("appearance_select"))
                            .on_click(move |_, window, cx| {
                                let index = cx
                                    .global::<theme::Catalog>()
                                    .packages
                                    .iter()
                                    .position(|package| package.id == id);
                                if let Some(index) = index {
                                    theme::apply(index, Some(window), cx);
                                } else {
                                    crate::feedback::error(
                                        "",
                                        &tr("appearance_unavailable"),
                                        window,
                                        cx,
                                    );
                                }
                            }),
                    )
                    .when(
                        self.updates.read(cx).available(&self.id) || pending,
                        |actions| {
                            actions.child(
                                Button::new("market-theme-update")
                                    .small()
                                    .primary()
                                    .label(tr("market_update"))
                                    .disabled(pending)
                                    .loading(pending)
                                    .on_click(move |_, window, cx| {
                                        updates.update(cx, |state, cx| {
                                            state.update(&update_id, window, cx)
                                        });
                                    }),
                            )
                        },
                    )
                    .into_any_element(),
            )
        } else if self.source.is_some() {
            Some(
                Button::new("market-theme-install")
                    .small()
                    .primary()
                    .label(tr("market_install"))
                    .disabled(pending || package.is_none())
                    .loading(pending)
                    .on_click(cx.listener(|view, _, window, cx| view.install(window, cx)))
                    .into_any_element(),
            )
        } else {
            None
        };
        let summary = super::summary(
            title,
            String::new(),
            description,
            Some(
                Icon::new(IconName::Palette)
                    .size(px(40.))
                    .into_any_element(),
            ),
            action,
            cx,
        );
        let content = if let Some(package) = package {
            self.content(&package, cx)
        } else if self.loading {
            crate::settings::market::details::skeleton(cx).into_any_element()
        } else {
            crate::feedback::retry("market-theme-reload")
                .on_click(cx.listener(|view, _, _, cx| view.load(cx)))
                .into_any_element()
        };
        super::panel("market_theme_details", summary, content, &self.scroll, cx)
    }
}
