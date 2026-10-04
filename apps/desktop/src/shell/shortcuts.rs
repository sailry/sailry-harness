use super::*;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenPage(pub Page);

actions!(sailry, [RefreshHost, CloseFocused]);

pub(crate) fn init(cx: &mut App) {
    #[derive(Default)]
    struct Installed;
    impl Global for Installed {}
    if cx.has_global::<Installed>() {
        return;
    }
    cx.set_global(Installed);
    crate::resources::init(cx);
    crate::terminal::init(cx);
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-,", OpenPage(Page::Settings), Some("Sailry")),
        KeyBinding::new("secondary-w", CloseFocused, Some("WorkspacePane")),
        KeyBinding::new("secondary-w", CloseFocused, Some("ResourcePanel")),
        KeyBinding::new("secondary-w", CloseFocused, Some("SailryTerminal")),
        KeyBinding::new("secondary-f", FindConversation, Some("SailryConversation")),
        KeyBinding::new(
            "secondary-f",
            FindConversation,
            Some("LiveConversation > Input"),
        ),
        KeyBinding::new(
            "secondary-f",
            FindConversation,
            Some("ConversationSearch > Input"),
        ),
    ]);
}

impl Shell {
    pub(super) fn open_page(
        &mut self,
        action: &OpenPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(action.0, window, cx);
    }
    pub(super) fn refresh_host(&mut self, _: &RefreshHost, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(live) = &mut self.live {
            let project = live.project;
            live.select(live.selected, cx);
            live.project = project;
        }
    }
}
