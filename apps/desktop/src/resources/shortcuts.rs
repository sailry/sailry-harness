//! Resource opening shortcuts are contributed by the captured session's catalog.
use super::launcher::Destination;
use crate::shell::Shell;
use gpui_kit::*;
use sailry_protocol::plugin::desktop::ResourceKind;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenResource(pub ResourceKind);

impl Shell {
    pub(crate) fn sync_resource_shortcuts(&self, cx: &mut App) {
        let context = self.resource_keys.read(cx).context();
        let predicate: std::rc::Rc<KeyBindingContextPredicate> =
            KeyBindingContextPredicate::parse(&context)
                .expect("generated resource context")
                .into();
        let bindings = self
            .resource_shortcuts(cx)
            .into_iter()
            .filter_map(|(resource, key)| {
                KeyBinding::load(
                    &key,
                    Box::new(OpenResource(resource)),
                    Some(predicate.clone()),
                    false,
                    None,
                    &DummyKeyboardMapper,
                )
                .ok()
            })
            .collect();
        self.resource_keys
            .update(cx, |keys, cx| keys.replace(bindings, cx));
    }

    pub(super) fn resource_shortcuts(&self, cx: &App) -> Vec<(ResourceKind, String)> {
        if self.page != crate::preview::Page::Conversation {
            return Vec::new();
        }
        self.current_chat()
            .map(|chat| chat.read(cx).renderer_shortcuts(cx))
            .unwrap_or_default()
    }

    pub(crate) fn open_resource_shortcut(
        &mut self,
        action: &OpenResource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Recheck the current captured catalog when an old dispatch is delayed.
        if !self
            .resource_shortcuts(cx)
            .iter()
            .any(|(resource, _)| *resource == action.0)
        {
            return;
        }
        self.open_destination(Destination::from(action.0), window, cx);
    }
}
