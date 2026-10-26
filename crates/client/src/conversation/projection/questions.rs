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
            return Err(sailry_protocol::Fault::invalid(
                "question identity or terminal answer changed",
            ));
        }
        *previous = question;
    } else {
        if page.questions.iter().any(|previous| {
            previous.turn == question.turn
                && previous.entry == question.entry
                && previous.index == question.index
        }) {
            return Err(sailry_protocol::Fault::invalid(
                "tool call has multiple questions",
            ));
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
            return Err(sailry_protocol::Fault::invalid(
                "conversation repeats a question",
            ));
        }
        check(page, question)?;
    }
    Ok(())
}

fn check(page: &Page, question: &Question) -> Result<(), Fault> {
    if question.session != page.session || question.entry.is_empty() {
        return Err(sailry_protocol::Fault::invalid(
            "question belongs to another conversation or has no call",
        ));
    }
    if let Some(entry) = page.entries.iter().find(|entry| entry.id == question.entry) {
        let Some(Part::ToolCall { name, summary, .. }) = entry.parts.get(question.index) else {
            return Err(sailry_protocol::Fault::invalid(
                "question does not reference a canonical tool call",
            ));
        };
        if entry.turn != question.turn
            || !matches!(
                name.as_str(),
                "ask_user" | "mcp_elicitation" | "plan_review"
            )
        {
            return Err(sailry_protocol::Fault::invalid(
                "question references another turn or tool",
            ));
        }
        let spec: &Spec = summary
            .question
            .as_deref()
            .ok_or_else(|| sailry_protocol::Fault::invalid("question has invalid input"))?;
        spec.validate()?;
        if name == "ask_user" && spec.prompt.is_empty() {
            return Err(sailry_protocol::Fault::invalid("question prompt is empty"));
        }
        if name == "plan_review" && (!matches!(spec.input, Input::Plan) || !spec.prompt.is_empty())
        {
            return Err(sailry_protocol::Fault::invalid(
                "plan review does not match its source",
            ));
        }
        if (name == "mcp_elicitation")
            != matches!(spec.input, Input::Form { .. } | Input::Url { .. })
        {
            return Err(sailry_protocol::Fault::invalid(
                "MCP input does not match its source",
            ));
        }
        if question.state == State::Declined
            && !matches!(spec.input, Input::Form { .. } | Input::Url { .. })
        {
            return Err(sailry_protocol::Fault::invalid(
                "decline requires an MCP request",
            ));
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
