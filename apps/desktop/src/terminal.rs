//! A mountable terminal view. Containers own navigation and PTY creation/closure.
mod connection;
mod effects;
mod graphics;
mod grid;
mod input;
mod keyboard;
mod links;
mod mouse;
mod search;
mod selection;
#[cfg(test)]
mod tests;
mod text;
mod viewport;

use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        scroll::Scrollbar,
        *,
    },
    *,
};
use sailry_client::terminal::View as ScreenView;
use sailry_protocol::{Command, terminal as protocol};

pub(crate) use connection::{Binding, Scope};
pub(crate) use input::{Copy, Paste, SelectAll};
pub(crate) use search::{Close, Find, Next, Previous};

pub(crate) fn init(cx: &mut App) {
    let (copy, paste, select) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-v", "cmd-a")
    } else {
        ("ctrl-shift-c", "ctrl-shift-v", "ctrl-shift-a")
    };
    cx.bind_keys([
        KeyBinding::new(copy, Copy, Some("SailryTerminal")),
        KeyBinding::new(paste, Paste, Some("SailryTerminal")),
        KeyBinding::new(select, SelectAll, Some("SailryTerminal")),
        KeyBinding::new("secondary-f", Find, Some("SailryTerminal")),
        KeyBinding::new("secondary-f", Find, Some("TerminalSearch > Input")),
        KeyBinding::new("secondary-g", Next, Some("SailryTerminal")),
        KeyBinding::new("secondary-g", Next, Some("TerminalSearch > Input")),
        KeyBinding::new("secondary-shift-g", Previous, Some("SailryTerminal")),
        KeyBinding::new(
            "secondary-shift-g",
            Previous,
            Some("TerminalSearch > Input"),
        ),
        KeyBinding::new("escape", Close, Some("TerminalSearch > Input")),
    ]);
}

pub(crate) struct View {
    binding: Binding,
    state: ScreenView,
    focus: FocusHandle,
    connection: connection::Connection,
    scroll: viewport::Scroll,
    metrics: viewport::Metrics,
    selection: selection::Selection,
    composition: input::Composition,
    keyboard: keyboard::State,
    cache: grid::Cache,
    graphics: graphics::Cache,
    search: search::Search,
    drag_point: Point<Pixels>,
    autoscroll: Option<Task<()>>,
    resize: Option<protocol::Viewport>,
    appearance: Option<protocol::Appearance>,
    mouse: mouse::State,
    message: Option<&'static str>,
    claim_pending: bool,
    cursor_visible: bool,
    text_visible: bool,
    focused: bool,
    window_active: bool,
    bell: bool,
    bell_reset: Option<Task<()>>,
    _subscription: Task<()>,
    _results: Task<()>,
    _focus: Subscription,
    _blur: Subscription,
    _blink: Task<()>,
}

impl View {
    #[cfg(test)]
    pub(crate) fn info(&self) -> Option<&protocol::Info> {
        self.state.snapshot.as_ref().map(|snapshot| &snapshot.info)
    }

    pub fn new(binding: Binding, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe(window, cx, |view: &Self, _| {
            view.message.into_iter().collect()
        });
        cx.observe_global::<crate::preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        cx.on_release_in(window, |view, window, _| view.graphics.clear(window))
            .detach();
        let search_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(tr("terminal_search_placeholder"))
        });
        cx.subscribe_in(&search_input, window, |view, _, event, _, cx| {
            view.search_changed(event, cx)
        })
        .detach();
        cx.observe_window_activation(window, |view, window, cx| {
            view.window_active = window.is_window_active();
            view.cursor_visible = true;
            if !view.window_active {
                for event in view.keyboard.release_all() {
                    view.input(protocol::Input::Key { event }, cx);
                }
            }
            if view.focused {
                view.report_focus(view.window_active, cx);
            }
            cx.notify();
        })
        .detach();
        let focus = cx.focus_handle();
        let (connection, mut screens, mut results) =
            connection::Connection::start(&binding, crate::theme::terminal(cx));
        let subscription = cx.spawn_in(window, async move |view, cx| {
            while screens.changed().await.is_ok() {
                let state = screens.borrow_and_update().clone();
                if view
                    .update_in(cx, |view, window, cx| {
                        let first = view.state.snapshot.is_none();
                        view.receive(state, cx);
                        if first && view.focused {
                            view.activate(window, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let completed = cx.spawn(async move |view, cx| {
            while let Some(result) = results.recv().await {
                if view
                    .update(cx, |view, cx| {
                        view.claim_pending = false;
                        view.message = result.err().map(|error| match error.code {
                            sailry_protocol::ErrorCode::OutcomeUnknown => "terminal_input_unknown",
                            _ => "terminal_input_failed",
                        });
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let focused = cx.on_focus(&focus, window, |view, window, cx| view.activate(window, cx));
        let blurred = cx.on_blur(&focus, window, |view, _, cx| {
            for event in view.keyboard.release_all() {
                view.input(protocol::Input::Key { event }, cx);
            }
            view.report_focus(false, cx);
            view.focused = false;
            view.cursor_visible = true;
            view.composition = Default::default();
            cx.notify();
        });
        let blink = cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(530))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        if view.state.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot
                                .screen
                                .rows
                                .iter()
                                .chain(&snapshot.screen.scrollback)
                                .any(|line| line.spans.iter().any(|span| span.style.blink))
                        }) {
                            view.text_visible = !view.text_visible;
                            cx.notify();
                        }
                        if view.cursor_focused()
                            && view
                                .state
                                .snapshot
                                .as_ref()
                                .and_then(|snapshot| snapshot.screen.cursor)
                                .is_some_and(|cursor| cursor.blinking)
                        {
                            view.cursor_visible = !view.cursor_visible;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            binding,
            state: ScreenView::default(),
            focus,
            connection,
            scroll: Default::default(),
            metrics: Default::default(),
            selection: Default::default(),
            composition: Default::default(),
            keyboard: Default::default(),
            cache: Default::default(),
            graphics: Default::default(),
            search: search::Search {
                input: search_input,
                open: false,
                query: String::new(),
                matches: Vec::new(),
                current: 0,
            },
            drag_point: point(px(0.), px(0.)),
            autoscroll: None,
            resize: None,
            appearance: None,
            mouse: Default::default(),
            message: None,
            claim_pending: false,
            cursor_visible: true,
            text_visible: true,
            focused: false,
            window_active: window.is_window_active(),
            bell: false,
            bell_reset: None,
            _subscription: subscription,
            _results: completed,
            _focus: focused,
            _blur: blurred,
            _blink: blink,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }

    fn update_screen(&mut self, state: ScreenView) {
        let previous = self
            .state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.screen.cursor);
        let cursor = state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.screen.cursor);
        // Background animation must not restart the cursor's blink phase.
        if cursor != previous {
            self.cursor_visible = true;
        }
        self.state = state;
        if self.search.open {
            self.refresh_search();
        }
    }

    fn controlling(&self) -> bool {
        self.state.connected
            && self.state.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.info.owner == Some(self.binding.caller)
                    && snapshot.info.status == protocol::Status::Running
            })
    }

    fn cursor_focused(&self) -> bool {
        self.focused && self.window_active && self.controlling()
    }

    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        self.message = (!self.connection.enqueue(command)).then_some("terminal_input_failed");
        self.cursor_visible = true;
        cx.notify();
    }

    fn input(&mut self, input: protocol::Input, cx: &mut Context<Self>) {
        if !self.controlling() {
            // Ownership and connection status come from the current snapshot.
            // A keystroke before the first frame must not leave a stale warning.
            cx.notify();
            return;
        }
        let edits = match &input {
            protocol::Input::Text { .. } | protocol::Input::Paste { .. } => true,
            protocol::Input::Key { event } => {
                event.action != protocol::Action::Release
                    && !matches!(
                        event.key,
                        protocol::Key::Named {
                            key: protocol::NamedKey::AltLeft
                                | protocol::NamedKey::AltRight
                                | protocol::NamedKey::ControlLeft
                                | protocol::NamedKey::ControlRight
                                | protocol::NamedKey::MetaLeft
                                | protocol::NamedKey::MetaRight
                                | protocol::NamedKey::ShiftLeft
                                | protocol::NamedKey::ShiftRight
                                | protocol::NamedKey::CapsLock
                                | protocol::NamedKey::Fn
                        }
                    )
            }
            _ => false,
        };
        let snapshot = self.state.snapshot.as_ref().unwrap();
        self.send(
            Command::InputTerminal {
                terminal: self.binding.id,
                revision: snapshot.info.revision,
                input,
            },
            cx,
        );
        if edits {
            self.selection.clear();
            self.scroll.bottom();
        }
    }

    fn activate(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.focused = true;
        self.cursor_visible = true;
        let Some(snapshot) = &self.state.snapshot else {
            return;
        };
        if self.controlling() {
            self.report_focus(true, cx);
            self.resize = None;
            self.appearance = None;
            cx.notify();
            return;
        }
        if snapshot.info.owner.is_none() {
            self.claim_control(cx);
        }
    }

    fn claim_control(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.state.snapshot else {
            return;
        };
        if self.state.connected
            && snapshot.info.status == protocol::Status::Running
            && snapshot.info.ssh.is_none()
            && !self.controlling()
            && !self.claim_pending
        {
            let command = Command::ClaimTerminal {
                terminal: self.binding.id,
                expected_revision: snapshot.info.revision,
            };
            self.claim_pending = true;
            self.send(command, cx);
        }
    }
}

impl Focusable for View {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let paint = view.clone();
        let screen = self.state.snapshot.clone();
        let foreground = screen.as_ref().map_or(cx.theme().foreground, |snapshot| {
            grid::color(snapshot.screen.foreground)
        });
        let status = self.status();
        v_flex()
            .id("terminal-component")
            .key_context("SailryTerminal")
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::next_match))
            .on_action(cx.listener(Self::previous_match))
            .debug_selector(|| "terminal-component".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .text_color(foreground)
            .relative()
            .child(
                div()
                    .id("terminal-grid")
                    .debug_selector(|| "terminal-grid".into())
                    .mx_2()
                    .mt_1()
                    .mb_2()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .overflow_hidden()
                    .key_context("SailryTerminal")
                    .track_focus(&self.focus)
                    .cursor(self.pointer_cursor(window))
                    .on_key_down(cx.listener(Self::key_down))
                    .on_key_up(cx.listener(Self::key_up))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(
                        |view, _: &gpui_kit::component::input::Copy, window, cx| {
                            view.copy(&Copy, window, cx)
                        },
                    ))
                    .when(self.controlling(), |grid| {
                        grid.on_action(cx.listener(
                            |view, _: &gpui_kit::component::input::Paste, window, cx| {
                                view.paste(&Paste, window, cx)
                            },
                        ))
                    })
                    .on_action(cx.listener(
                        |view, _: &gpui_kit::component::input::SelectAll, window, cx| {
                            view.select_all(&SelectAll, window, cx)
                        },
                    ))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::context_menu))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::middle_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Right, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::mouse_up))
                    .on_hover(cx.listener(|_, _, _, cx| cx.notify()))
                    .on_mouse_move(cx.listener(Self::mouse_move))
                    .on_scroll_wheel(cx.listener(Self::wheel))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                view.update(cx, |view, cx| grid::prepare(view, bounds, window, cx))
                            },
                            move |bounds, drawing, window, cx| {
                                grid::paint(&paint, bounds, drawing, window, cx)
                            },
                        )
                        .size_full(),
                    )
                    .child(Scrollbar::vertical(&self.scroll)),
            )
            .when(self.search.open, |element| {
                element.child(
                    div()
                        .absolute()
                        .top_1()
                        .left_2()
                        .right_2()
                        .flex()
                        .justify_end()
                        .child(self.search_bar(cx)),
                )
            })
            .when(self.bell, |element| {
                element.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(cx.theme().foreground),
                )
            })
            .when_some(status, |element, status| {
                element.child(
                    v_flex()
                        .id("terminal-status")
                        .debug_selector(|| "terminal-status".into())
                        .absolute()
                        .inset_0()
                        .occlude()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .bg(cx.theme().background.opacity(0.9))
                        .text_color(cx.theme().muted_foreground)
                        .text_sm()
                        .child(tr(status))
                        .when(self.connection.can_retry(), |element| {
                            element.child(
                                Button::new("terminal-retry-open")
                                    .small()
                                    .outline()
                                    .label(tr("chat_retry"))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.connection.retry();
                                        view.state.error = None;
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(status == "terminal_other_controller", |element| {
                            element.child(
                                Button::new("terminal-take-control")
                                    .debug_selector(|| "terminal-take-control".into())
                                    .small()
                                    .outline()
                                    .label(tr("terminal_take_control"))
                                    .disabled(self.claim_pending)
                                    .on_click(cx.listener(Self::take_control)),
                            )
                        }),
                )
            })
    }
}

impl View {
    fn status(&self) -> Option<&'static str> {
        if self.state.error.is_some() {
            return Some("terminal_unavailable");
        }
        if !self.state.connected || self.state.snapshot.is_none() {
            return Some("terminal_connecting");
        }
        let snapshot = self.state.snapshot.as_ref().unwrap();
        if snapshot.info.status != protocol::Status::Running {
            return Some("terminal_exited");
        }
        if !self.controlling() {
            return Some(if snapshot.info.ssh.is_some() {
                "terminal_unavailable"
            } else {
                "terminal_other_controller"
            });
        }
        None
    }

    fn take_control(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.activate(window, cx);
        self.claim_control(cx);
    }
}
