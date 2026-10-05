use super::*;
use core::prelude::v1::test;
use sailry_protocol::{Command, Output, plugin::desktop::ResourceKind};

#[gpui::test]
fn selected_provider_needs_no_shortcut(cx: &mut TestAppContext) {
    let fixture = crate::plugins::fixture::Fixture::new(false);
    let Output::Plugin(mut first) = fixture.execute(Command::ReadPlugin {
        name: "files".into(),
    }) else {
        panic!("Files package expected")
    };
    let mut second = first.clone();
    first.summary.name = "a-first".into();
    second.summary.name = "z-second".into();
    first
        .extension
        .as_mut()
        .unwrap()
        .desktop
        .as_mut()
        .unwrap()
        .renderers[0]
        .shortcut = None;
    second
        .extension
        .as_mut()
        .unwrap()
        .desktop
        .as_mut()
        .unwrap()
        .renderers[0]
        .shortcut = Some("secondary-j".into());
    cx.update(|cx| {
        let registry = cx.new(|cx| Registry::bind(&fixture.binding, Surface::Composer, cx));
        let metadata = registry.read(cx).metadata.clone();
        metadata.update(cx, |metadata, _| {
            metadata
                .entries
                .insert(first.summary.name.clone(), first.clone());
            metadata
                .entries
                .insert(second.summary.name.clone(), second.clone());
        });
        assert!(registry.read(cx).renderer_shortcuts(cx).is_empty());
        metadata.update(cx, |metadata, _| {
            metadata.entries.get_mut("a-first").unwrap().summary.enabled = false
        });
        assert_eq!(
            registry.read(cx).renderer_shortcuts(cx),
            [(ResourceKind::Documents, "secondary-j".into())]
        );
        metadata.update(cx, |metadata, _| {
            metadata
                .entries
                .get_mut("z-second")
                .unwrap()
                .extension
                .as_mut()
                .unwrap()
                .scope = Scope::Desktop
        });
        assert!(registry.read(cx).renderer_shortcuts(cx).is_empty());
    });
    fixture.close();
}
