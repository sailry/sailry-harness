use super::*;

impl Registry {
    pub(super) fn indicator(&self, entry: Entry, cx: &mut Context<Self>) -> AnyElement {
        let key = entry.key.clone();
        let id = format!("plugin-control-{}-{}", key.package.name, key.id);
        let indicator = serde_json::from_value::<ui::Indicator>(entry.state.value.clone()).ok();
        let hint: SharedString = indicator
            .as_ref()
            .map(|value| value.hint.clone())
            .unwrap_or_else(|| "0".into())
            .into();
        let percent = indicator.as_ref().and_then(|value| value.percent);
        let color = match indicator.as_ref().map(|value| value.tone) {
            Some(ui::Tone::Warning) => cx.theme().warning,
            Some(ui::Tone::Danger) => cx.theme().danger,
            _ => cx.theme().muted_foreground,
        };
        let content =
            progress::ProgressCircle::new(SharedString::from(format!("indicator-{}", key.id)))
                .value(percent.unwrap_or(0.).min(100.) as f32)
                .loading(indicator.as_ref().is_some_and(|value| value.loading))
                .with_size(px(20.))
                .color(color)
                .accessibility_label(hint.clone());
        if entry.declaration.handler.is_none() {
            return div()
                .id(SharedString::from(id))
                .flex_shrink_0()
                .px_1()
                .tooltip(move |window, cx| tooltip::Tooltip::new(hint.clone()).build(window, cx))
                .child(content)
                .into_any_element();
        }
        let Some(mounted) = self
            .mounted
            .get(&key.package.name)
            .filter(|mounted| mounted.package == key.package)
        else {
            return div().into_any_element();
        };
        let button = Button::new(SharedString::from(id.clone()))
            .ghost()
            .small()
            .rounded_full()
            .px_1()
            .flex_shrink_0()
            .disabled(!entry.state.enabled)
            .accessibility_label(hint.clone())
            .tooltip(hint)
            .debug_selector(move || id.clone())
            .child(content);
        crate::theme::popover(
            popover::Popover::new(SharedString::from(format!(
                "popover-{}-{}",
                key.package.name, key.id
            ))),
            cx,
        )
        .open(indicator.as_ref().is_some_and(|value| value.open))
        .anchor(Anchor::BottomRight)
        .trigger(button)
        .child(mounted.panel.clone())
        .on_open_change(cx.listener(move |registry, open: &bool, window, cx| {
            registry.invoke(&key, EventKind::Change, (*open).into(), window, cx)
        }))
        .into_any_element()
    }
}
