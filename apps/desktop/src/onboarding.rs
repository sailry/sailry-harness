//! Local welcome guide; every connection action reuses the actual provider editor.
use crate::{
    backend::Services,
    preferences,
    preview::Page,
    settings::{Section, Workspace},
    shell::Shell,
    tr,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        dialog::{DialogDescription, DialogTitle},
        scroll::ScrollableElement,
        *,
    },
    *,
};

actions!(sailry, [ShowWelcome]);

impl Shell {
    pub(crate) fn show_welcome(
        &mut self,
        _: &ShowWelcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_welcome(window, cx);
    }

    pub(crate) fn open_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.try_global::<Services>().is_none() || window.has_active_dialog(cx) {
            return;
        }
        let shell = cx.entity().downgrade();
        let scroll = ScrollHandle::new();
        window.open_dialog(cx, move |dialog, window, cx| {
            let width = (window.viewport_size().width - px(48.)).min(px(520.));
            dialog
                .p_0()
                .w(width)
                .max_h(window.viewport_size().height * 0.85)
                .rounded(cx.theme().radius_lg)
                .overflow_hidden()
                .bg(cx.theme().popover)
                .text_color(cx.theme().popover_foreground)
                .overlay_closable(false)
                .on_ok(|_, _, _| false)
                .on_close(|_, _, cx| preferences::mark_welcome_seen(cx))
                .content({
                    let shell = shell.clone();
                    let scroll = scroll.clone();
                    move |content, window, cx| {
                        content.min_h_0().gap_0().child(body(
                            shell.clone(),
                            scroll.clone(),
                            window,
                            cx,
                        ))
                    }
                })
        });
    }

    fn welcome_provider(
        &mut self,
        brand: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = cx.try_global::<Services>().cloned() else {
            return;
        };
        let target = self.live.as_ref().and_then(|live| {
            live.transport_for(live.selected)
                .map(|transport| (transport, live.name(live.selected)))
        });
        let Some((transport, label)) = target else {
            crate::feedback::error("", &tr("onboarding_host_unavailable"), window, cx);
            return;
        };
        self.navigate(Page::Settings, window, cx);
        self.settings.update(cx, |settings, cx| {
            settings.bind_providers(transport, services.runtime, label, cx);
            settings.select(Section::Providers, cx);
        });
        Workspace::add_provider(self.settings.clone(), brand, window, cx);
    }
}

fn launch(
    shell: &WeakEntity<Shell>,
    brand: Option<&'static str>,
    window: &mut Window,
    cx: &mut App,
) {
    preferences::mark_welcome_seen(cx);
    window.close_dialog(cx);
    let _ = shell.update(cx, |shell, cx| shell.welcome_provider(brand, window, cx));
}

fn body(shell: WeakEntity<Shell>, scroll: ScrollHandle, window: &Window, cx: &App) -> AnyElement {
    let width = (window.viewport_size().width - px(48.)).min(px(520.));
    let hero_height = px(280.).min(window.viewport_size().height * 0.4);
    let logo_size = px(96.).min(hero_height * 0.55);
    let brands = Workspace::provider_brands();
    let columns = if width >= px(450.) {
        5
    } else if width >= px(330.) {
        4
    } else {
        3
    };
    let button = shell.clone();
    v_flex()
        .id("welcome-guide")
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .child(
            div()
                .w_full()
                .h(hero_height)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                // Decorative artwork uses GPUI's supported gradient and theme colors.
                .bg(linear_gradient(
                    115.,
                    linear_color_stop(cx.theme().blue.opacity(0.5), 0.),
                    linear_color_stop(cx.theme().magenta.opacity(0.5), 1.),
                ))
                .child(
                    div()
                        .rounded(cx.theme().radius_lg)
                        .bg(gpui_kit::white())
                        .child(img("branding/sailry.png").size(logo_size)),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .p_6()
                .gap_5()
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            DialogTitle::new()
                                .text_2xl()
                                .font_semibold()
                                .child(tr("onboarding_title")),
                        )
                        .child(DialogDescription::new().child(tr("onboarding_description"))),
                )
                .child(div().grid().grid_cols(columns).w_full().gap_2().children(
                    brands.into_iter().map(|(brand, icon)| {
                        let shell = shell.clone();
                        Button::new(SharedString::from(format!("welcome-{brand}")))
                            .ghost()
                            .h_auto()
                            .p_3()
                            .accessibility_label(tr(brand))
                            .tooltip(tr(brand))
                            .child(icon.size_8())
                            .on_click(move |_, window, cx| launch(&shell, Some(brand), window, cx))
                    }),
                ))
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("welcome-add")
                                .primary()
                                .w_full()
                                .large()
                                .label(tr("onboarding_add"))
                                .on_click(move |_, window, cx| launch(&button, None, window, cx)),
                        )
                        .child(
                            Button::new("welcome-later")
                                .ghost()
                                .w_full()
                                .large()
                                .label(tr("onboarding_later"))
                                .on_click(|_, window, cx| {
                                    preferences::mark_welcome_seen(cx);
                                    window.close_dialog(cx);
                                }),
                        ),
                ),
        )
        .vertical_scrollbar(&scroll)
        .into_any_element()
}
