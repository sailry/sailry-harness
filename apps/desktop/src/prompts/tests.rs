use super::*;

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
    fn buttons_and_selection() {
        for (scenario, check) in [
            (
                "choices_and_dismissal",
                (|_scenario: &str| {
                    let labels = ["Cancel", "Discard", "Save"].map(str::to_owned);
                    assert!(
                        matches!(buttons(&labels), rfd::MessageButtons::YesNoCancelCustom(yes, no, cancel)
                    if yes == "Save" && no == "Discard" && cancel == "Cancel"),
                        "choices and dismissal"
                    );
                    for (index, label) in labels.iter().enumerate() {
                        assert_eq!(
                            selection(&labels, Custom(label.clone())),
                            Some(index),
                            "choices and dismissal"
                        );
                    }
                    assert_eq!(selection(&labels, Cancel), None, "choices and dismissal");
                    assert_eq!(selection(&labels, Yes), Some(2), "choices and dismissal");
                    assert_eq!(selection(&labels, No), Some(1), "choices and dismissal");
                    assert_eq!(
                        selection(&labels, Custom("Unknown".into())),
                        None,
                        "choices and dismissal"
                    );
                }) as fn(&str),
            ),
            (
                "acceptance_only",
                (|_scenario: &str| {
                    let labels = ["Cancel", "Delete"].map(str::to_owned);
                    assert!(
                        matches!(buttons(&labels), rfd::MessageButtons::OkCancelCustom(ok, cancel)
                    if ok == "Delete" && cancel == "Cancel"),
                        "acceptance only"
                    );
                    assert_eq!(
                        selection(&labels, Custom("Delete".into())),
                        Some(1),
                        "acceptance only"
                    );
                    assert_eq!(
                        selection(&labels, Custom("Cancel".into())),
                        Some(0),
                        "acceptance only"
                    );
                    assert_eq!(selection(&labels, Cancel), None, "acceptance only");
                    assert_eq!(selection(&labels, Ok), Some(1), "acceptance only");
                    assert_eq!(selection(&labels, Yes), None, "acceptance only");
                }),
            ),
            (
                "closes_an_alert",
                (|_scenario: &str| {
                    let labels = ["Close".to_owned()];
                    assert!(
                        matches!(buttons(&labels), rfd::MessageButtons::OkCustom(ok) if ok == "Close"),
                        "closes an alert"
                    );
                    assert_eq!(
                        selection(&labels, Custom("Close".into())),
                        Some(0),
                        "closes an alert"
                    );
                    assert_eq!(selection(&labels, Ok), Some(0), "closes an alert");
                    assert_eq!(selection(&labels, Cancel), None, "closes an alert");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
