use super::{
    tests::{appearance, viewport},
    *,
};

#[test]
fn reports_requested_effects() {
    let mut vt = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    assert!(
        vt.encode_input(&Input::Focus { focused: true })
            .unwrap()
            .is_empty()
    );
    vt.write(b"\x1b]2;build\x07\x1b]7;file://localhost/tmp/project\x07\x07\x1b]52;c;aGVsbG8=\x07\x1b[?1004h\x1b[?1007h").unwrap();
    let features = vt.checkpoint().features;
    assert_eq!(features.title, "build");
    assert_eq!(features.directory, "file://localhost/tmp/project");
    assert_eq!(features.bell, 1);
    assert_eq!(features.clipboard.unwrap().text, "hello");
    assert!(features.alternate_scroll);
    assert_eq!(
        vt.encode_input(&Input::Focus { focused: true }).unwrap(),
        b"\x1b[I"
    );
    assert_eq!(
        vt.encode_input(&Input::Focus { focused: false }).unwrap(),
        b"\x1b[O"
    );
    vt.write(b"\x1b[?1004l\x1b[?1007l\x1b]52;c;?\x07").unwrap();
    assert!(!vt.checkpoint().features.alternate_scroll);
    assert!(!vt.checkpoint().features.focus_reporting);
    assert!(
        vt.encode_input(&Input::Focus { focused: true })
            .unwrap()
            .is_empty()
    );
    assert_eq!(vt.checkpoint().features.clipboard.unwrap().sequence, 1);
}

#[test]
fn retains_graphics_without_retransmission() {
    let mut vt = Vt::new(&viewport(80, 4), &appearance(ColorScheme::Dark), 100).unwrap();
    vt.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=7,c=2,r=2;/wAA/w==\x1b\\")
        .unwrap();
    let graphics = vt.checkpoint().graphics;
    assert_eq!(graphics.images.len(), 1);
    assert_eq!(graphics.placements.len(), 1);
    assert_eq!(graphics.images[0].width, 1);
    assert_eq!(graphics.placements[0].image, 7);
    let update = vt.write(b"hello").unwrap();
    assert!(
        matches!(update.screen, Some(ScreenUpdate::Patch { patch }) if patch.graphics.is_none())
    );
    vt.write(b"\r\n\r\n\r\n\r\n\r\n").unwrap();
    let graphics = vt.checkpoint().graphics;
    assert_eq!(graphics.images.len(), 1);
    assert!(graphics.placements[0].row < 0);
    vt.write(b"\x1b_Ga=d,d=A\x1b\\").unwrap();
    assert!(vt.checkpoint().graphics.images.is_empty());
}

#[test]
fn decodes_png_transfers() {
    let mut vt = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    vt.write(b"\x1b_Ga=T,f=100,i=9;iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==\x1b\\").unwrap();
    assert_eq!(vt.checkpoint().graphics.images.len(), 1);
}

#[test]
fn projects_unicode_placeholders() {
    let mut vt = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    vt.write(b"\x1b_Ga=T,U=1,f=32,s=1,v=1,i=7,c=2,r=1;/wAA/w==\x1b\\")
        .unwrap();
    vt.write("\x1b[38;2;0;0;7m\u{10eeee}\u{305}\u{305}\u{10eeee}\x1b[0m".as_bytes())
        .unwrap();
    let graphics = vt.checkpoint().graphics;
    assert_eq!(graphics.images.len(), 1);
    assert_eq!(graphics.placements.len(), 2);
    assert_eq!(graphics.placements[0].image, 7);
    assert_eq!(graphics.placements[0].source, [0, 0, 1, 1]);
    assert_eq!(graphics.placements[1].source, [0, 0, 1, 1]);
    assert_eq!(graphics.placements[0].tile, Some([0, 0, 2, 1]));
    assert_eq!(graphics.placements[1].tile, Some([1, 0, 2, 1]));
}

#[test]
fn retains_offscreen_images() {
    let mut vt = Vt::new(&viewport(80, 4), &appearance(ColorScheme::Dark), 100).unwrap();
    vt.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=7,c=2,r=2,q=2;/wAA/w==\x1b\\\r\n\r\n\r\n\r\n\r\n")
        .unwrap();
    let graphics = vt.checkpoint().graphics;
    assert_eq!(graphics.images.len(), 1);
    assert_eq!(graphics.placements.len(), 1);
    assert!(graphics.placements[0].row < 0);
}
