use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    input::{Textarea, TextareaState},
};
use std::time::Duration;

struct Review {
    browser: WeakEntity<Browser>,
    target: Target,
    element: Element,
    image: Arc<[u8]>,
    preview: Arc<Image>,
    note: Entity<TextareaState>,
    pending: bool,
    closed: bool,
    task: Option<Task<()>>,
}

pub(super) fn open(
    browser: Entity<Browser>,
    target: Target,
    element: Element,
    image: Arc<[u8]>,
    window: &mut Window,
    cx: &mut App,
) {
    let review = cx.new(|cx| {
        cx.observe(&browser, |_, _, cx| cx.notify()).detach();
        Review {
            browser: browser.downgrade(),
            target,
            element,
            preview: Arc::new(Image::from_bytes(ImageFormat::Jpeg, image.to_vec())),
            image,
            note: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(tr("browser_annotation_note"))
                    .auto_grow(2, 5)
            }),
            pending: false,
            closed: false,
            task: None,
        }
    });
    show(review, window, cx);
}

fn show(review: Entity<Review>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = review.clone();
        let cancel = review.clone();
        let save = review.clone();
        let pending = review.read(cx).pending;
        let disabled = pending || review.read(cx).target.stop.is_cancelled();
        dialog
            .form_title(tr("browser_annotation"))
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| close.update(cx, |review, cx| review.close(cx)))
            .child(review.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("browser-annotation-cancel")
                            .label(tr("settings_cancel"))
                            .debug_selector(|| "browser-annotation-cancel".into())
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |review, cx| review.close(cx));
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("browser-annotation-add")
                            .primary()
                            .label(tr("browser_annotation_add"))
                            .disabled(disabled)
                            .loading(pending)
                            .debug_selector(|| "browser-annotation-add".into())
                            .on_click(move |_, window, cx| {
                                save.update(cx, |review, cx| review.submit(window, cx))
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests;

impl Review {
    fn close(&mut self, cx: &mut Context<Self>) {
        self.closed = true;
        self.target.stop.cancel();
        self.task = None;
        if let Some(browser) = self.browser.upgrade() {
            browser.update(cx, |browser, cx| {
                if browser
                    .annotation
                    .target
                    .as_ref()
                    .is_some_and(|target| target.token == self.target.token)
                {
                    browser.cancel_annotation(cx);
                    browser.changed(cx);
                }
            });
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending {
            return;
        }
        let Some(browser) = self
            .browser
            .upgrade()
            .filter(|browser| browser.read(cx).annotation_matches(&self.target))
        else {
            crate::feedback::error("", &tr("browser_annotation_stale"), window, cx);
            return;
        };
        let note = self.note.read(cx).value();
        let reference = match self.element.reference(note.as_ref()) {
            Ok(value) => value,
            Err(key) => {
                crate::feedback::error("", &tr(key), window, cx);
                return;
            }
        };
        let (sender, receive) = tokio::sync::oneshot::channel();
        browser.update(cx, |_, cx| {
            cx.emit(Transfer {
                reference,
                image: self.image.clone(),
                stop: self.target.stop.clone(),
                reply: RefCell::new(Some(sender)),
            })
        });
        self.pending = true;
        self.task = Some(cx.spawn_in(window, async move |review, cx| {
            let timer = cx.background_executor().timer(Duration::from_secs(5));
            let result = tokio::select! {
                response = receive => response.unwrap_or(Err("browser_annotation_stale")),
                _ = timer => Err("browser_annotation_stale"),
            };
            let _ = review.update_in(cx, |review, window, cx| {
                if review.closed {
                    return;
                }
                review.pending = false;
                review.task = None;
                match result {
                    Ok(()) => {
                        review.close(cx);
                        window.close_dialog(cx);
                    }
                    Err(key) => {
                        crate::feedback::error("", &tr(key), window, cx);
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }
}

impl Render for Review {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .whitespace_normal()
                    .child(self.element.url.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .font_family(cx.theme().mono_font_family.clone())
                    .whitespace_normal()
                    .child(self.element.selector.clone()),
            )
            .child(
                img(self.preview.clone())
                    .w_full()
                    .max_h(px(240.))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .id("browser-annotation-text")
                    .max_h(px(100.))
                    .overflow_y_scroll()
                    .child(self.element.text.clone()),
            )
            .children(self.element.truncated.then(|| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("browser_annotation_truncated"))
            }))
            .child(
                div()
                    .id("browser-annotation-note")
                    .test_support()
                    .debug_selector(|| "browser-annotation-note".into())
                    .child(Textarea::new(&self.note)),
            )
    }
}
