//! Local welcome guide; every connection action reuses the actual provider editor.
use crate::theme::DialogStyle as _;
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

impl Shell {
    pub(crate) fn open_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.try_global::<Services>().is_none() || window.has_active_dialog(cx) {
            return;
        }
        let shell = cx.entity().downgrade();
        let client = self
            .live
            .as_ref()
            .and_then(|live| live.transport_for(live.selected))
            .map(|transport| std::sync::Arc::new(sailry_client::Client::new(transport)));
        let guide = cx.new(|cx| {
            let mut guide = Guide {
                shell: shell.clone(),
                scroll: ScrollHandle::new(),
                entries: None,
                task: None,
            };
            guide.load(client, cx);
            guide
        });
        window.open_dialog(cx, move |dialog, window, cx| {
            let width = (window.viewport_size().width - px(48.)).min(px(520.));
            dialog
                .p_0()
                .w(width)
                .max_h(window.viewport_size().height * 0.85)
                .rounded(cx.theme().radius_lg)
                .overflow_hidden()
                .floating_surface(cx)
                .overlay_closable(false)
                .on_ok(|_, _, _| false)
                .on_close(|_, _, cx| preferences::mark_welcome_seen(cx))
                .content({
                    let guide = guide.clone();
                    move |content, _, _| content.min_h_0().gap_0().child(guide.clone())
                })
        });
    }

    fn welcome_provider(
        &mut self,
        brand: Option<String>,
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

const BRAND_TILE: Pixels = px(44.);
const BRAND_GAP: Pixels = px(4.);
/// Brand glyphs stay compact inside the ghost tiles.
const BRAND_ICON: Rems = rems(1.375);

fn launch(shell: &WeakEntity<Shell>, brand: Option<String>, window: &mut Window, cx: &mut App) {
    preferences::mark_welcome_seen(cx);
    window.close_dialog(cx);
    let _ = shell.update(cx, |shell, cx| shell.welcome_provider(brand, window, cx));
}

fn body(
    shell: WeakEntity<Shell>,
    scroll: ScrollHandle,
    entries: Option<&[sailry_protocol::conversation::presets::Entry]>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let width = (window.viewport_size().width - px(48.)).min(px(520.));
    let hero_height = px(280.).min(window.viewport_size().height * 0.4);
    let logo_size = px(96.).min(hero_height * 0.55);
    let brands = Workspace::provider_brands(entries, BRAND_ICON);
    // Balance brand tiles across the fewest rows the content width allows.
    let fit = (((width - px(48.) + BRAND_GAP) / (BRAND_TILE + BRAND_GAP)).floor() as usize).max(1);
    let per_row = brands
        .len()
        .div_ceil(brands.len().div_ceil(fit).max(1))
        .max(1);
    let mut tiles: Vec<_> = brands
        .into_iter()
        .map(|(brand, label, icon)| {
            let shell = shell.clone();
            Button::new(SharedString::from(format!("welcome-{brand}")))
                .ghost()
                .size(BRAND_TILE)
                .p_0()
                .accessibility_label(label.clone())
                .tooltip(label)
                .child(icon)
                .on_click(move |_, window, cx| launch(&shell, Some(brand.clone()), window, cx))
        })
        .collect();
    let mut rows = Vec::new();
    while !tiles.is_empty() {
        let rest = tiles.split_off(per_row.min(tiles.len()));
        rows.push(h_flex().justify_center().gap(BRAND_GAP).children(tiles));
        tiles = rest;
    }
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
                // Content masks are rectangular, so the hero repeats the dialog's top radius.
                .rounded_t(cx.theme().radius_lg)
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
                        .size(logo_size)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .rounded(cx.theme().radius_lg)
                        .shadow_lg()
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
                .child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap(BRAND_GAP)
                        .children(rows),
                )
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
                                .secondary()
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

struct Guide {
    shell: WeakEntity<Shell>,
    scroll: ScrollHandle,
    entries: Option<Vec<sailry_protocol::conversation::presets::Entry>>,
    task: Option<Task<()>>,
}

impl Guide {
    fn load(
        &mut self,
        client: Option<std::sync::Arc<sailry_client::Client>>,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = client else {
            return;
        };
        let Some(services) = cx.try_global::<Services>().cloned() else {
            return;
        };
        let job = services.runtime.spawn(read(Some(client)));
        self.task = Some(cx.spawn(async move |guide, cx| {
            let result = job.await;
            let _ = guide.update(cx, |guide, cx| {
                if let Ok(Ok(Some(listing))) = result {
                    guide.entries = Some(listing.entries);
                    cx.notify();
                }
            });
        }));
    }
}

impl Render for Guide {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        body(
            self.shell.clone(),
            self.scroll.clone(),
            self.entries.as_deref(),
            window,
            cx,
        )
    }
}

async fn read(
    client: Option<std::sync::Arc<sailry_client::Client>>,
) -> Result<Option<sailry_protocol::conversation::presets::Listing>, sailry_protocol::Fault> {
    let Some(client) = client else {
        return Ok(None);
    };
    match client
        .execute(
            client.prepare(sailry_protocol::Command::ListChannelPresets {
                refresh_partners: false,
            }),
        )
        .await?
    {
        sailry_protocol::Output::ChannelPresets(listing) => Ok(Some(listing)),
        _ => Err(sailry_protocol::Fault::invalid(
            "channel directory expected",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preview_keeps_directory_local() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(runtime.block_on(read(None)).unwrap().is_none());
    }
}
