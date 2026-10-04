use super::*;
use core::prelude::v1::test;
use std::time::{Duration, Instant};

pub(crate) fn wait(cx: &mut VisualTestContext) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if let Some(prompt) = cx.pending_prompt() {
            return prompt;
        }
        assert!(Instant::now() < deadline, "native prompt did not appear");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn answer(cx: &mut VisualTestContext, key: &str) {
    wait(cx);
    cx.simulate_prompt_answer(&tr(key));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
}

// GPUI's test platform supplies simulated OS responses without opening dialogs.
pub(super) fn show(
    level: PromptLevel,
    title: &str,
    detail: &str,
    choices: &[SharedString],
    window: &mut Window,
    cx: &mut App,
) -> Task<rfd::MessageDialogResult> {
    let labels = choices.to_vec();
    let buttons: Vec<_> = choices
        .iter()
        .enumerate()
        .map(|(index, label)| {
            if choices.len() > 1 && index == 0 {
                PromptButton::cancel(label.clone())
            } else {
                PromptButton::new(label.clone())
            }
        })
        .collect();
    let response = window.prompt(level, title, Some(detail), &buttons, cx);
    window.spawn(cx, async move |_| {
        response
            .await
            .ok()
            .and_then(|index| labels.get(index))
            .map(|label| rfd::MessageDialogResult::Custom(label.to_string()))
            .unwrap_or(rfd::MessageDialogResult::Cancel)
    })
}

mod responses {
    use super::*;
    use core::prelude::v1::test;
    use rfd::MessageDialogResult::*;

    #[test]
    fn choices_and_dismissal() {
        let labels = ["Cancel", "Discard", "Save"].map(str::to_owned);
        assert!(
            matches!(buttons(&labels), rfd::MessageButtons::YesNoCancelCustom(yes, no, cancel)
            if yes == "Save" && no == "Discard" && cancel == "Cancel")
        );
        for (index, label) in labels.iter().enumerate() {
            assert_eq!(selection(&labels, Custom(label.clone())), Some(index));
        }
        assert_eq!(selection(&labels, Cancel), None);
        assert_eq!(selection(&labels, Yes), Some(2));
        assert_eq!(selection(&labels, No), Some(1));
        assert_eq!(selection(&labels, Custom("Unknown".into())), None);
    }

    #[test]
    fn acceptance_only() {
        let labels = ["Cancel", "Delete"].map(str::to_owned);
        assert!(
            matches!(buttons(&labels), rfd::MessageButtons::OkCancelCustom(ok, cancel)
            if ok == "Delete" && cancel == "Cancel")
        );
        assert_eq!(selection(&labels, Custom("Delete".into())), Some(1));
        assert_eq!(selection(&labels, Custom("Cancel".into())), Some(0));
        assert_eq!(selection(&labels, Cancel), None);
        assert_eq!(selection(&labels, Ok), Some(1));
        assert_eq!(selection(&labels, Yes), None);
    }

    #[test]
    fn closes_an_alert() {
        let labels = ["Close".to_owned()];
        assert!(matches!(buttons(&labels), rfd::MessageButtons::OkCustom(ok) if ok == "Close"));
        assert_eq!(selection(&labels, Custom("Close".into())), Some(0));
        assert_eq!(selection(&labels, Ok), Some(0));
        assert_eq!(selection(&labels, Cancel), None);
    }
}

struct Content;
impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn serializes_and_releases(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_, visual) = cx.add_window_view(|_, _| Content);
    let first = visual.update(|window, cx| {
        let first = ask(
            PromptLevel::Warning,
            "Delete",
            "A file",
            &["Cancel".into(), "Delete".into()],
            window,
            cx,
        );
        assert!(active(window, cx));
        let duplicate = ask(
            PromptLevel::Info,
            "Duplicate",
            "",
            &["Close".into()],
            window,
            cx,
        );
        drop(duplicate);
        assert!(active(window, cx));
        first
    });
    assert_eq!(wait(visual).0, "Delete");
    visual.simulate_prompt_answer("Cancel");
    visual.run_until_parked();
    assert!(!visual.update(|window, cx| active(window, cx)));
    drop(first);
    let second = visual
        .update(|window, cx| ask(PromptLevel::Info, "Next", "", &["Close".into()], window, cx));
    assert_eq!(wait(visual).0, "Next");
    visual.simulate_prompt_answer("Close");
    visual.run_until_parked();
    assert!(!visual.update(|window, cx| active(window, cx)));
    drop(second);
    visual.update(|window, _| window.remove_window());
}
