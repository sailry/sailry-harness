use super::*;

pub(super) fn check(cx: &mut VisualTestContext, view: &Entity<View>, root: &std::path::Path) {
    let expected = [
        b"\x1b[97;;97u".as_slice(),
        b"\x1b[97;1:2;97u",
        b"\x1b[97;1:3u",
        b"\x1b[13;2u",
        b"\x1b[13;2:3u",
        b"\x1b[97:65;2;65u",
        b"\x1b[97:65;2:3u",
        "你好".as_bytes(),
        b"\x1b[200~xyz\x1b[201~",
    ]
    .concat();
    let editing = if cfg!(target_os = "macos") {
        b"\x01\x05\x1bb\x1bf\x15".as_slice()
    } else {
        b""
    };
    let script = format!(
        "stty raw -echo\nprintf 'EDIT_READY'\ndd bs=1 count={} of=editing.bytes 2>/dev/null\ndd bs=1 count=1 of=handoff.bytes 2>/dev/null\nprintf '\\033[>31u\\033[?2004hKITTY_READY'\ndd bs=1 count={} of=kitty.bytes 2>/dev/null\nprintf '\\033[<u\\033[?2004lKEYS_DONE\\r\\n'\nstty sane\n",
        editing.len(),
        expected.len()
    );
    std::fs::write(root.join("keyboard.sh"), script).unwrap();
    // Simulated shell typing does not synthesize key releases.
    view.update(cx, |view, _| {
        view.keyboard.release_all();
    });
    paste(cx, "/bin/sh ./keyboard.sh");
    cx.simulate_keystrokes("enter");
    wait(cx, view, |view| content(view).contains("EDIT_READY"));
    if cfg!(target_os = "macos") {
        for key in [
            "cmd-left",
            "cmd-right",
            "alt-left",
            "alt-right",
            "cmd-backspace",
        ] {
            cx.simulate_keystrokes(key);
            release(cx, key);
        }
    }
    // The shared input queue delivers this marker after every editing key-up.
    // Keep Kitty disabled until then so a delayed release stays in legacy mode.
    view.update(cx, |view, cx| {
        view.input(protocol::Input::Text { text: "!".into() }, cx);
    });
    wait(cx, view, |view| content(view).contains("KITTY_READY"));
    cx.simulate_keystrokes("a");
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke {
            key_char: Some("a".into()),
            ..Keystroke::parse("a").unwrap()
        },
        is_held: true,
        prefer_character_input: false,
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_text_in_range(None, "a", window, cx)
        })
    });
    release(cx, "a");
    cx.simulate_keystrokes("shift-enter");
    release(cx, "shift-enter");
    cx.simulate_keystrokes("shift-a");
    release(cx, "shift-a");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "nihao", Some(5..5), window, cx);
            view.replace_text_in_range(None, "你好", window, cx);
        })
    });
    paste(cx, "xyz");
    wait(cx, view, |view| content(view).contains("KEYS_DONE"));
    assert_eq!(std::fs::read(root.join("editing.bytes")).unwrap(), editing);
    assert_eq!(std::fs::read(root.join("handoff.bytes")).unwrap(), b"!");
    assert_eq!(std::fs::read(root.join("kitty.bytes")).unwrap(), expected);
    // The pop restores ordinary shell input after the TUI exits.
    paste(cx, "printf 'legacy-%s\\n' restored");
    cx.simulate_keystrokes("enter");
    wait(cx, view, |view| content(view).contains("legacy-restored"));
}

fn release(cx: &mut VisualTestContext, key: &str) {
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse(key).unwrap(),
    });
}
