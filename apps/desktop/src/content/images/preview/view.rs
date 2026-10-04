use super::*;

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let failed = matches!(self.result, Some(Err(_)));
        let editable = matches!(self.result, Some(Ok(_))) && !self.transforming;
        let viewport = window.viewport_size();
        let spacing = cx.theme().spacing_tokens();
        let toolbar_height = px(40.);
        let canvas = size(
            (viewport.width - spacing.lg * 2.).max(px(1.)),
            (viewport.height - spacing.lg * 2. - spacing.md - toolbar_height).max(px(1.)),
        );
        let motion = cx.theme().motion_tokens().clone();
        let presence = Presence::new(ElementId::from(("lightbox", cx.entity_id())), !self.closing)
            .transition(Transition::new(motion.duration_normal))
            .sample(window, cx);
        if !presence.should_render() {
            cx.defer_in(window, |view, window, cx| {
                view.clear(Some(window), cx);
                if view.focus.contains_focused(window, cx) {
                    window.close_dialog(cx);
                }
            });
        }
        let zoom = spring(
            ElementId::from(("image-zoom", cx.entity_id())),
            self.viewport.zoom,
            motion.spring_control,
            window,
            cx,
        );
        let image_size = self
            .result
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|image| {
                let fitted = viewport::fit(image.size(0), canvas);
                let scale = zoom * (0.96 + 0.04 * presence.progress);
                size(fitted.width * scale, fitted.height * scale)
            });
        if let Some(image_size) = image_size {
            self.viewport.clamp_pan(image_size, canvas);
        }
        v_flex()
            .id("attachment-image-preview")
            .debug_selector(|| "attachment-image-preview".into())
            .aria_label(tr("chat_image_preview"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .gap(spacing.md)
            .p(spacing.lg)
            .w(viewport.width)
            .h(viewport.height)
            .min_w_0()
            .items_center()
            .bg(cx.theme().overlay.alpha(0.94))
            .opacity(presence.progress)
            .child(
                div()
                    .debug_selector(|| "image-lightbox-canvas".into())
                    .flex()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .cursor_grab()
                    .on_scroll_wheel(cx.listener(Self::wheel))
                    .on_pinch(cx.listener(Self::pinch))
                    .on_mouse_move(cx.listener(Self::drag))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.viewport.drag = None;
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            view.viewport.drag = None;
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, window, cx| {
                            view.focus.focus(window, cx);
                            view.viewport.drag = Some(event.position);
                        }),
                    )
                    .child(match &self.result {
                        Some(Ok(image)) => img(image.clone())
                            .debug_selector(|| "image-lightbox-image".into())
                            .flex_shrink_0()
                            .relative()
                            .left(self.viewport.pan.x)
                            .top(self.viewport.pan.y)
                            .w(image_size.unwrap().width)
                            .h(image_size.unwrap().height)
                            .object_fit(ObjectFit::Contain)
                            .into_any_element(),
                        Some(Err(key)) => div()
                            .text_sm()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().popover)
                            .text_color(cx.theme().popover_foreground)
                            .child(tr(key))
                            .into_any_element(),
                        None => Progress::new("image-loading")
                            .w_32()
                            .loading(true)
                            .accessibility_label(tr("chat_image_loading"))
                            .into_any_element(),
                    }),
            )
            .child(gpui_kit::component::surface::render(
                h_flex()
                    .debug_selector(|| "image-lightbox-toolbar".into())
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .p_1()
                    .h(toolbar_height)
                    .rounded(cx.theme().radius_full())
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .when(self.sources.len() > 1, |row| {
                        row.child(
                            Button::new("image-previous")
                                .debug_selector(|| "image-previous".into())
                                .ghost()
                                .small()
                                .size_8()
                                .rounded(cx.theme().radius_full())
                                .icon(IconName::ChevronLeft)
                                .disabled(self.index == 0)
                                .tooltip(tr("chat_image_previous"))
                                .accessibility_label(tr("chat_image_previous"))
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.step(false, window, cx)),
                                ),
                        )
                        .child(
                            Button::new("image-next")
                                .debug_selector(|| "image-next".into())
                                .ghost()
                                .small()
                                .size_8()
                                .rounded(cx.theme().radius_full())
                                .icon(IconName::ChevronRight)
                                .disabled(self.index + 1 >= self.sources.len())
                                .tooltip(tr("chat_image_next"))
                                .accessibility_label(tr("chat_image_next"))
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.step(true, window, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new("image-zoom-out")
                            .debug_selector(|| "image-zoom-out".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(IconName::Minus)
                            .disabled(!editable || self.viewport.zoom <= MIN_ZOOM)
                            .tooltip(tr("chat_image_zoom_out"))
                            .accessibility_label(tr("chat_image_zoom_out"))
                            .on_click(cx.listener(|view, _, _, cx| view.zoom_by(0.8, cx))),
                    )
                    .child(
                        Button::new("image-zoom-in")
                            .debug_selector(|| "image-zoom-in".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(IconName::Plus)
                            .disabled(!editable || self.viewport.zoom >= MAX_ZOOM)
                            .tooltip(tr("chat_image_zoom_in"))
                            .accessibility_label(tr("chat_image_zoom_in"))
                            .on_click(cx.listener(|view, _, _, cx| view.zoom_by(1.25, cx))),
                    )
                    .child(
                        Button::new("image-fit")
                            .debug_selector(|| "image-fit".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(IconName::Minimize)
                            .disabled(!editable)
                            .tooltip(tr("chat_image_fit"))
                            .accessibility_label(tr("chat_image_fit"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.viewport = Viewport::default();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("image-rotate")
                            .debug_selector(|| "image-rotate".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(IconName::RotateCw)
                            .disabled(!editable)
                            .tooltip(tr("chat_image_rotate"))
                            .accessibility_label(tr("chat_image_rotate"))
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.transform(Change::Rotate, window, cx)
                            })),
                    )
                    .child(
                        Button::new("image-flip-horizontal")
                            .debug_selector(|| "image-flip-horizontal".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(Icon::default().path("icons/flip-horizontal.svg"))
                            .tooltip(tr("chat_image_flip_horizontal"))
                            .accessibility_label(tr("chat_image_flip_horizontal"))
                            .disabled(!editable)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.transform(Change::Horizontal, window, cx)
                            })),
                    )
                    .child(
                        Button::new("image-flip-vertical")
                            .debug_selector(|| "image-flip-vertical".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(Icon::default().path("icons/flip-vertical.svg"))
                            .tooltip(tr("chat_image_flip_vertical"))
                            .accessibility_label(tr("chat_image_flip_vertical"))
                            .disabled(!editable)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.transform(Change::Vertical, window, cx)
                            })),
                    )
                    .when(failed, |row| {
                        row.child(
                            Button::new("image-retry")
                                .debug_selector(|| "image-retry".into())
                                .ghost()
                                .small()
                                .size_8()
                                .rounded(cx.theme().radius_full())
                                .icon(IconName::Redo)
                                .tooltip(tr("chat_retry"))
                                .accessibility_label(tr("chat_retry"))
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.reload(window, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new("image-download")
                            .debug_selector(|| "image-download".into())
                            .ghost()
                            .small()
                            .size_8()
                            .rounded(cx.theme().radius_full())
                            .icon(IconName::ArrowDown)
                            .disabled(self.sources.is_empty())
                            .tooltip(tr("files_download"))
                            .accessibility_label(tr("files_download"))
                            .on_click(cx.listener(|view, _, window, cx| {
                                if let Some(source) = view.sources.get(view.index) {
                                    source.download(
                                        view.binding.client.clone(),
                                        view.binding.runtime.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            })),
                    ),
                cx,
            ))
    }
}
