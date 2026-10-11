use super::*;

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn bounds(visual: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    visual
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap()
}

fn sizes(visual: &mut VisualTestContext, id: QuestionId) -> [Pixels; 2] {
    let handle = visual.update(|window, _| window.window_handle());
    [440., 900.].map(|width| {
        visual.simulate_window_resize(handle, size(px(width), px(1100.)));
        draw(visual);
        let row = bounds(visual, format!("live-question-history-{id}"));
        let summary = bounds(visual, format!("live-question-history-{id}-summary"));
        let label = bounds(visual, format!("live-question-history-{id}-summary-label"));
        assert!(row.size.width > px(0.));
        assert!(row.left() >= px(0.));
        assert!(row.right() <= px(width));
        assert!(summary.left() >= row.left());
        assert!(summary.right() <= row.right());
        assert!(label.left() > row.left());
        assert!(label.right() <= row.right());
        assert!(label.size.height > px(24.));
        assert!(row.size.height >= label.size.height);
        assert!(label.bottom() <= row.bottom());
        label.size.height
    })
}

#[gpui::test]
fn wraps_long_choice_labels(cx: &mut TestAppContext) {
    init(cx);
    let options = [
        "删除当前项目的记录，保留本地文件和已有会话，不要删除工作目录中的源代码、配置文件或尚未提交的修改。 Preserve the full option through its final words 中文 🙂".to_owned(),
        "Keep the project".to_owned(),
    ];
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![(
                "ask_user".into(),
                json!({"prompt": "Choose a project action", "input": {"kind": "choice", "options": options, "multiple": false, "allow_other": false}}),
            )],
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let id = ready(&view, visual, None);
        let handle = visual.update(|window, _| window.window_handle());
        for width in [440., 900.] {
            visual.simulate_window_resize(handle, size(px(width), px(1100.)));
            draw(visual);
            let row = bounds(visual, "live-question-choice-0".into());
            let label = bounds(visual, "radio-label-0".into());
            let indicator = bounds(visual, "radio-indicator-0".into());
            let short = bounds(visual, "radio-label-1".into());
            assert!(label.left() > indicator.right());
            assert!(label.right() <= row.right());
            assert!(label.bottom() <= row.bottom());
            assert!(label.size.height > short.size.height);
            assert!((indicator.center().y - label.top() - short.size.height * 0.5).abs() <= px(1.));
        }
        view.read_with(visual, |view, _| {
            assert!(matches!(
                &view.questions.editing.as_ref().unwrap().spec.input,
                Input::Choice { options: stored, .. } if stored == &options
            ));
        });
        tap(visual, "live-question-choice-0");
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .questions
                .editing
                .as_ref()
                .unwrap()
                .answer(cx)),
            Some(Answer::Choices {
                selected: vec![0],
                other: None,
            })
        );
        tap(visual, "live-question-submit");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).questions.pending
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.questions[0].id, id);
            assert_eq!(
                page.questions[0].state,
                State::Answered(Answer::Choices {
                    selected: vec![0],
                    other: None,
                })
            );
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn wraps_pending_and_resolved_prompts(cx: &mut TestAppContext) {
    init(cx);
    let prompt = "Created the requested project directory. The selected command was not found in this environment. Please provide the project address or initialization command so the remaining setup uses the intended package. This question retains the full response, including Unicode 中文 🙂 and the final sentence.".to_owned();
    let answer = "Use the existing project and preserve its files. The chosen repository contains the initialization instructions and the package manifest. Keep the supplied settings and finish the task after reviewing those instructions. 中文 🙂 ".repeat(2);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![(
                "ask_user".into(),
                json!({"prompt": prompt, "input": {"kind": "text", "multiline": true, "max_bytes": 4096}}),
            )],
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let id = ready(&view, visual, None);
        let opened = sizes(visual, id);
        assert!(opened[0] > opened[1]);
        visual.simulate_keystrokes("escape");
        visual.executor().advance_clock(Duration::from_millis(400));
        wait(visual, |cx| !view.read(cx).questions.open);
        let pending = sizes(visual, id);
        assert_eq!(opened, pending);
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let call = view
                .history
                .calls
                .iter()
                .find(|call| {
                    call.question
                        .as_ref()
                        .is_some_and(|question| question.id == id)
                })
                .unwrap();
            assert_eq!(call.question_spec(page).unwrap().prompt, prompt);
            assert_eq!(call.question.as_ref().unwrap().state, State::Pending);
        });
        tap(visual, "live-question-trigger");
        ready(&view, visual, None);
        tap(visual, "live-question-input");
        visual.simulate_input(&answer);
        tap(visual, "live-question-submit");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).questions.pending
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(
                page.questions[0].state,
                State::Answered(Answer::Text(answer.clone()))
            );
        });
        assert!(history(visual, id).visible());
        let resolved = sizes(visual, id);
        assert!(resolved[0] > resolved[1]);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-question-answer-{id}").into_boxed_str()
                ))
                .is_none()
        );
        toggle_history(visual, id);
        draw(visual);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-question-answer-{id}").into_boxed_str()
                ))
                .is_some()
        );
        let expanded = sizes(visual, id);
        assert_eq!(expanded, resolved);
        toggle_history(visual, id);
        draw(visual);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-question-answer-{id}").into_boxed_str()
                ))
                .is_none()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
