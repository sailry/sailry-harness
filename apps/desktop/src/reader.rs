//! System voices stay on the controller; no text reaches a model or execution Node.
use gpui_kit::*;
use std::time::Duration;

#[derive(Default)]
struct Service {
    engine: Option<tts::Tts>,
    current: Option<String>,
    monitor: Option<Task<()>>,
}
impl Global for Service {}
impl Drop for Service {
    fn drop(&mut self) {
        if let Some(engine) = &mut self.engine {
            let _ = engine.stop();
        }
    }
}

pub(crate) fn active(id: &str, cx: &App) -> bool {
    cx.try_global::<Service>()
        .is_some_and(|service| service.current.as_deref() == Some(id))
}

pub(crate) fn toggle(id: String, markdown: &str, window: &mut Window, cx: &mut App) {
    if !cx.has_global::<Service>() {
        cx.set_global(Service::default());
    }
    let result = if active(&id, cx) {
        stop(cx)
    } else {
        speak(id, markdown, cx)
    };
    if result.is_err() {
        crate::feedback::error("", &crate::tr("reader_failed"), window, cx);
    }
    cx.refresh_windows();
}

fn stop(cx: &mut App) -> Result<(), tts::Error> {
    let service = cx.global_mut::<Service>();
    if let Some(engine) = &mut service.engine {
        engine.stop()?;
    }
    service.current = None;
    service.monitor = None;
    Ok(())
}

fn speak(id: String, markdown: &str, cx: &mut App) -> Result<(), tts::Error> {
    let text = text(markdown);
    if text.trim().is_empty() {
        return Ok(());
    }
    let service = cx.global_mut::<Service>();
    if service.engine.is_none() {
        service.engine = Some(engine()?);
    }
    let engine = service
        .engine
        .as_mut()
        .expect("system voice was initialized");
    engine.speak(text, true)?;
    service.current = Some(id.clone());
    let monitor = cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let done = cx.update(|cx| {
                let service = cx.global_mut::<Service>();
                if service.current.as_ref() != Some(&id) {
                    return true;
                }
                if service
                    .engine
                    .as_ref()
                    .is_none_or(|engine| !engine.is_speaking().unwrap_or(false))
                {
                    service.current = None;
                    cx.refresh_windows();
                    return true;
                }
                false
            });
            if done {
                break;
            }
        }
    });
    cx.global_mut::<Service>().monitor = Some(monitor);
    Ok(())
}

fn engine() -> Result<tts::Tts, tts::Error> {
    // Sailry requires macOS 13; avoid tts 0.26.3 parsing a localized OS version string.
    #[cfg(target_os = "macos")]
    {
        tts::Tts::new(tts::Backends::AvFoundation)
    }
    #[cfg(not(target_os = "macos"))]
    {
        tts::Tts::default()
    }
}

fn text(markdown: &str) -> String {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let mut text = String::new();
    for event in Parser::new(markdown) {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(&value),
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) => text.push('\n'),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn preserves_words_without_markdown_syntax() {
        let spoken =
            text("# 中文标题\n\nHello **world**, [docs](https://example.com).\n\n`value`\n");
        assert!(spoken.contains("中文标题\n"));
        assert!(spoken.contains("Hello world, docs."));
        assert!(spoken.contains("value"));
        assert!(!spoken.contains("https://") && !spoken.contains("**"));
    }
    struct Controls;

    impl Render for Controls {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use gpui_kit::component::button::{Button, ButtonVariants};
            div().children(["first", "second"].map(|id| {
                Button::new(id)
                    .debug_selector(move || format!("reader-{id}"))
                    .ghost()
                    .label(id)
                    .on_click(move |_, window, cx| {
                        toggle(id.into(), "Sailry system voice", window, cx)
                    })
            }))
        }
    }

    #[gpui::test]
    #[ignore = "requires the native controller system voice"]
    fn native_start_stop_and_replace(cx: &mut TestAppContext) {
        use gpui_kit::component::Root;
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            let mut engine = engine().expect("native system voice");
            engine.set_volume(0.0).unwrap();
            cx.set_global(Service {
                engine: Some(engine),
                current: None,
                monitor: None,
            });
        });
        let (_, visual) = cx.add_window_view(|window, cx| {
            let controls = cx.new(|_| Controls);
            Root::new(controls, window, cx)
        });
        fn click(cx: &mut VisualTestContext, selector: &'static str) {
            cx.update(|window, cx| {
                window.refresh();
                _ = window.draw(cx);
            });
            let bounds = cx.debug_bounds(selector).expect("reader control");
            cx.simulate_click(bounds.center(), Modifiers::default());
        }
        for _ in 0..3 {
            click(visual, "reader-first");
            assert!(visual.update(|_, cx| active("first", cx)));
            click(visual, "reader-first");
            visual.update(|_, cx| {
                assert!(!active("first", cx));
                assert!(cx.global::<Service>().monitor.is_none());
            });
        }
        click(visual, "reader-first");
        click(visual, "reader-second");
        visual.update(|_, cx| {
            assert!(!active("first", cx));
            assert!(active("second", cx));
        });
        click(visual, "reader-second");
        visual.update(|window, cx| {
            assert!(!active("second", cx));
            assert!(cx.global::<Service>().monitor.is_none());
            _ = window.draw(cx);
            window.remove_window();
        });
    }
}
