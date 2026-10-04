//! Additional windows share the process Node owner and keep independent layouts.
use super::*;

impl Shell {
    pub(super) fn session_window(
        &self,
        session: sailry_protocol::Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else {
            return;
        };
        let hosts = live.hosts.clone();
        let node = live.selected;
        let observed = live.view.clone();
        let options = crate::window_options(cx);
        let result = cx.open_window(options, move |window, cx| {
            #[cfg(target_os = "macos")]
            crate::macos::configure_backdrop(window);
            window.set_window_title(&tr("app"));
            let shell = cx.new(|cx| {
                let mut shell = Shell::secondary(window, cx);
                let live = shell.live.as_mut().unwrap();
                live.hosts = hosts;
                live.select(node, cx);
                live.accept_view(observed);
                shell.reveal_session(session, window, cx);
                shell
            });
            cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
        });
        if let Err(error) = result {
            eprintln!("could not open session window: {error}");
            crate::feedback::toast(
                window,
                tr("session_window_failed"),
                Notification::error(tr("session_window_failed")),
                cx,
            );
        }
    }
}
