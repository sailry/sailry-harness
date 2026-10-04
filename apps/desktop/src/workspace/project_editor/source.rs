use super::*;
use gpui_kit::component::{input::InputEvent, notification::Notification};

#[cfg(test)]
mod tests;

impl Editor {
    pub(super) fn select_source(
        &mut self,
        clone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.clone == clone {
            return;
        }
        let inactive = std::mem::replace(&mut self.other_path, self.inputs[1].read(cx).value());
        self.clone = clone;
        self.inputs[1].update(cx, |input, cx| input.set_value(inactive, window, cx));
        self.error = None;
        self.suggest(window, cx);
        cx.notify();
    }

    pub(super) fn observe_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [self.inputs[1].clone(), self.repository.clone()] {
            self.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |editor, _, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        editor.suggest(window, cx);
                    }
                },
            ));
        }
    }

    pub(super) fn suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = if self.clone {
            &self.repository
        } else {
            &self.inputs[1]
        };
        let value = source.read(cx).value();
        let name = value
            .trim()
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\', ':'])
            .next()
            .unwrap_or_default();
        let name = if self.clone {
            name.strip_suffix(".git").unwrap_or(name)
        } else {
            name
        };
        let current = self.inputs[0].read(cx).value();
        if current.trim().is_empty() || current == self.suggestion {
            self.inputs[0].update(cx, |input, cx| input.set_value(name.to_owned(), window, cx));
        }
        self.suggestion = name.to_owned();
        cx.notify();
    }

    pub(super) fn destination(&self, cx: &App) -> Result<String, &'static str> {
        let path = self.inputs[1].read(cx).value();
        if !self.clone {
            return Ok(path.trim().into());
        }
        let name = self.inputs[0].read(cx).value();
        let name = name.trim();
        if name.is_empty()
            || name.contains(['/', '\\', ':', '\0'])
            || matches!(name, "." | "..")
            || self.repository.read(cx).value().trim().is_empty()
            || path.trim().is_empty()
        {
            return Err("project_clone_required");
        }
        // The path belongs to the selected Node, not the controller's OS.
        let separator = if path.contains('\\') { '\\' } else { '/' };
        Ok(format!(
            "{}{}{}",
            path.trim().trim_end_matches(separator),
            separator,
            name
        ))
    }

    pub(super) fn fail(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.error = Some(key);
        crate::feedback::toast(
            window,
            tr(key),
            Notification::error(tr(key)).id::<Self>(),
            cx,
        );
        cx.notify();
    }
}
