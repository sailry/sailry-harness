//! The welcome guide reuses the same Node directory and channel configuration flow.
use crate::theme::DialogStyle as _;
use crate::{
    backend::Services,
    preferences,
    preview::Page,
    settings::{ReturnTo, Section, Workspace},
    shell::Shell,
    tr,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        dialog::{DialogClose, DialogDescription, DialogTitle},
        scroll::ScrollableElement,
        *,
    },
    *,
};
use std::rc::Rc;

const WIDTH: Pixels = px(640.);

impl Shell {
    pub(crate) fn open_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.try_global::<Services>().is_none() || window.has_active_dialog(cx) {
            return;
        }
        let shell = cx.entity().downgrade();
        let entries = self.live.as_ref().map(|_| Vec::new());
        let node = self.live.as_ref().map(|live| live.selected);
        let client = self
            .live
            .as_ref()
            .and_then(|live| live.transport_for(live.selected))
            .map(|transport| std::sync::Arc::new(sailry_client::Client::new(transport)));
        let guide = cx.new(|cx| {
            let mut guide = Guide {
                shell,
                node,
                scroll: ScrollHandle::new(),
                entries,
                task: None,
            };
            guide.load(client, cx);
            guide
        });
        show(guide, window, cx);
    }

    fn welcome_provider(
        &mut self,
        brand: String,
        return_to: ReturnTo,
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
        Workspace::add_provider(
            self.settings.clone(),
            Some(brand),
            Some(return_to),
            window,
            cx,
        );
    }
}

fn show(guide: Entity<Guide>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let width = (window.viewport_size().width - px(48.)).min(WIDTH);
        dialog
            .p_0()
            .w(width)
            .max_h(window.viewport_size().height * 0.85)
            .floating_surface(cx)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .close_button(false)
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .on_close(|_, _, cx| preferences::mark_welcome_seen(cx))
            .content({
                let guide = guide.clone();
                move |content, _, _| content.min_h_0().gap_0().child(guide.clone())
            })
    });
}

fn launch(guide: Entity<Guide>, brand: String, window: &mut Window, cx: &mut App) {
    let shell = guide.read(cx).shell.clone();
    let resume = guide.clone();
    let return_to = Rc::new(move |window: &mut Window, cx: &mut App| {
        let shell = resume.read(cx).shell.clone();
        let node = resume.read(cx).node;
        let _ = shell.update(cx, |shell, cx| {
            if shell.live.as_ref().map(|live| live.selected) == node {
                show(resume.clone(), window, cx);
            } else {
                shell.open_welcome(window, cx);
            }
        });
    });
    preferences::mark_welcome_seen(cx);
    window.close_dialog(cx);
    let _ = shell.update(cx, |shell, cx| {
        shell.welcome_provider(brand, return_to.clone(), window, cx)
    });
}

struct Guide {
    shell: WeakEntity<Shell>,
    node: Option<sailry_protocol::NodeId>,
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
        let width = (window.viewport_size().width - px(48.)).min(WIDTH);
        let guide = cx.entity().downgrade();
        v_flex()
            .id("welcome-guide")
            .relative()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(div().absolute().top(px(25.)).right(px(25.)).size_8().child(
                DialogClose::new().trigger(|button| button.ghost().size_8().icon(IconName::Close)),
            ))
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .pt(px(37.))
                    .px_9()
                    .child(img("branding/sailry-mark.svg").size(px(50.)))
                    .child(
                        DialogTitle::new()
                            .mt(px(19.))
                            .text_2xl()
                            .font_semibold()
                            .text_center()
                            .child(tr("onboarding_title")),
                    )
                    .child(
                        DialogDescription::new()
                            .mt(px(9.))
                            .text_center()
                            .child(tr("onboarding_description")),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .pt(px(35.))
                    .pb(px(40.))
                    .child(Workspace::provider_matrix(
                        self.entries.as_deref(),
                        width,
                        move |brand, window, cx| {
                            if let Some(guide) = guide.upgrade() {
                                launch(guide, brand, window, cx);
                            }
                        },
                    )),
            )
            .child(
                v_flex()
                    .px_9()
                    .pb_6()
                    .gap_2()
                    .items_center()
                    .child(
                        DialogDescription::new()
                            .text_center()
                            .child(tr("telemetry_welcome")),
                    )
                    .child(
                        Button::new("welcome-privacy-settings")
                            .ghost()
                            .small()
                            .label(tr("settings"))
                            .on_click(|_, window, cx| {
                                preferences::mark_welcome_seen(cx);
                                window.close_dialog(cx);
                                window.dispatch_action(
                                    Box::new(crate::settings::OpenSection(Section::General)),
                                    cx,
                                );
                            }),
                    ),
            )
            .vertical_scrollbar(&self.scroll)
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
