use super::*;

pub(super) fn check(cx: &mut VisualTestContext, view: &Entity<View>, root: &std::path::Path) {
    let script = "stty raw -echo\nprintf '\\033]2;Terminal effects\\007\\033]7;file://localhost/tmp\\007\\033]52;c;aGVsbG8=\\007\\007\\033_Ga=T,f=32,s=1,v=1,i=7,c=2,r=2,q=2;/wAA/w==\\033\\\\\\033[?1004hFOCUS_READY'\ndd bs=1 count=6 of=focus.bytes 2>/dev/null\nprintf '\\033[?1004lFOCUS_DONE\\r\\n'\nstty sane\n";
    std::fs::write(root.join("effects.sh"), script).unwrap();
    paste(cx, "/bin/sh ./effects.sh");
    cx.simulate_keystrokes("enter");
    wait(cx, view, |view| content(view).contains("FOCUS_READY"));
    view.read_with(cx, |view, cx| {
        let snapshot = view.state.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.info.title.as_deref(), Some("Terminal effects"));
        assert_eq!(
            snapshot.info.directory.as_deref(),
            Some("file://localhost/tmp")
        );
        assert_eq!(snapshot.screen.graphics.placements.len(), 1);
        assert!(
            view.graphics.image(7).is_some(),
            "terminal image was not decoded for drawing"
        );
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("hello")
        );
    });
    let grid = cx.debug_bounds("terminal-grid").unwrap();
    let viewport = view.read_with(cx, |view, _| view.resize.clone());
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    });
    wait(cx, view, |view| content(view).contains("FOCUS_DONE"));
    assert_eq!(cx.debug_bounds("terminal-grid").unwrap(), grid);
    let search = cx.debug_bounds("terminal-search").unwrap();
    assert!(search.size.width <= px(420.));
    assert_eq!(search.right(), grid.right());
    assert_eq!(view.read_with(cx, |view, _| view.resize.clone()), viewport);
    assert_eq!(
        std::fs::read(root.join("focus.bytes")).unwrap(),
        b"\x1b[I\x1b[O"
    );
    cx.simulate_keystrokes("escape");
    wait(cx, view, |view| !view.search.open);
    paste(cx, "printf '\\033_Ga=d,d=A\\033\\\\' ");
    cx.simulate_keystrokes("enter");
    wait(cx, view, |view| {
        view.state
            .snapshot
            .as_ref()
            .unwrap()
            .screen
            .graphics
            .images
            .is_empty()
    });
}
