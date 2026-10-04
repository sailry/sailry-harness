use super::*;
use conversation::question::{Answer, Input, Question, Spec, State};

pub(super) fn apply(page: &mut Page, question: Question) -> Result<(), Fault> {
    check(page, &question)?;
    if let Some(previous) = page
        .questions
        .iter_mut()
        .find(|previous| previous.id == question.id)
    {
        if previous.turn != question.turn
            || previous.entry != question.entry
            || previous.index != question.index
            || previous.state != State::Pending && previous.state != question.state
        {
            return Err(invalid("question identity or terminal answer changed"));
        }
        *previous = question;
    } else {
        if page.questions.iter().any(|previous| {
            previous.turn == question.turn
                && previous.entry == question.entry
                && previous.index == question.index
        }) {
            return Err(invalid("tool call has multiple questions"));
        }
        page.questions.push(question);
    }
    Ok(())
}

pub(super) fn validate(page: &Page) -> Result<(), Fault> {
    let mut ids = std::collections::BTreeSet::new();
    let mut calls = std::collections::BTreeSet::new();
    for question in &page.questions {
        if !ids.insert(question.id)
            || !calls.insert((question.turn, &question.entry, question.index))
        {
            return Err(invalid("conversation repeats a question"));
        }
        check(page, question)?;
    }
    Ok(())
}

fn check(page: &Page, question: &Question) -> Result<(), Fault> {
    if question.session != page.session || question.entry.is_empty() {
        return Err(invalid(
            "question belongs to another conversation or has no call",
        ));
    }
    if let Some(entry) = page.entries.iter().find(|entry| entry.id == question.entry) {
        let Some(Part::ToolCall {
            name, arguments, ..
        }) = entry.parts.get(question.index)
        else {
            return Err(invalid("question does not reference a canonical tool call"));
        };
        if entry.turn != question.turn || !matches!(name.as_str(), "ask_user" | "mcp_elicitation") {
            return Err(invalid("question references another turn or tool"));
        }
        let spec: Spec = serde_json::from_value(arguments.clone())
            .map_err(|_| invalid("question has invalid arguments"))?;
        spec.validate()?;
        if (name == "mcp_elicitation")
            != matches!(spec.input, Input::Form { .. } | Input::Url { .. })
        {
            return Err(invalid("MCP input does not match its source"));
        }
        if question.state == State::Declined
            && !matches!(spec.input, Input::Form { .. } | Input::Url { .. })
        {
            return Err(invalid("decline requires an MCP request"));
        }
        if let State::Answered(answer) = &question.state {
            match (&spec.input, answer) {
                (Input::Plan, Answer::Plan { turn }) if *turn != question.turn => {}
                _ => spec.validate_answer(answer)?,
            }
        }
    }
    Ok(())
}
