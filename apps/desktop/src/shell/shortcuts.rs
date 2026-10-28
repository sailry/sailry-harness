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
        KeyBinding::load("secondary-q", Box::new(Quit), None, false, None, cx)
            .expect("static shortcut"),
        KeyBinding::load(
            "secondary-,",
            Box::new(OpenPage(Page::Settings)),
            Some(
                KeyBindingContextPredicate::parse("Sailry")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-w",
            Box::new(CloseFocused),
            Some(
                KeyBindingContextPredicate::parse("WorkspacePane")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-w",
            Box::new(CloseFocused),
            Some(
                KeyBindingContextPredicate::parse("ResourcePanel")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-w",
            Box::new(CloseFocused),
            Some(
                KeyBindingContextPredicate::parse("SailryTerminal")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-f",
            Box::new(FindConversation),
            Some(
                KeyBindingContextPredicate::parse("SailryConversation")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-f",
            Box::new(FindConversation),
            Some(
                KeyBindingContextPredicate::parse("LiveConversation > Input")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
        KeyBinding::load(
            "secondary-f",
            Box::new(FindConversation),
            Some(
                KeyBindingContextPredicate::parse("ConversationSearch > Input")
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut"),
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
