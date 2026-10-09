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
        service.engine = Some(tts::Tts::default()?);
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
    #[gpui::test]
    #[ignore = "requires the native controller system voice"]
    fn initializes_native_system_voice(_: &mut TestAppContext) {
        let mut engine = tts::Tts::default().expect("native system voice");
        engine.set_volume(0.0).unwrap();
        engine.speak("Sailry system voice", true).unwrap();
        engine.stop().unwrap();
    }
}
