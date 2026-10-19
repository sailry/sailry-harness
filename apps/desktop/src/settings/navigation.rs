use super::Section;
use crate::{preview::Page, shell::Shell};
use gpui_kit::{Action, Context, Entity, Window};

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenDetails(pub Entity<super::PluginDetails>);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct CloseDetails;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenSection(pub Section);

impl Shell {
    pub(crate) fn open_extension_details(
        &mut self,
        action: &OpenDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
        cx.observe(&action.0, |_, _, cx| cx.notify()).detach();
        self.settings_details = Some(action.0.clone());
        cx.notify();
    }

    pub(crate) fn close_extension_details(
        &mut self,
        _: &CloseDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
    }

    pub(crate) fn clear_extension_details(&mut self, cx: &mut Context<Self>) {
        if let Some(details) = self.settings_details.take() {
            details.read(cx).cancel();
        }
        cx.notify();
    }

    pub(crate) fn open_settings_section(
        &mut self,
        action: &OpenSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(
            if action.0.is_extension() {
                Page::Plugins
            } else {
                Page::Settings
            },
            window,
            cx,
        );
        self.settings
            .update(cx, |settings, cx| settings.select(action.0, cx));
    }
}
