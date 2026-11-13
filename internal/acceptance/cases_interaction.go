package acceptance

import (
	"encoding/json"
	"fmt"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func caseApprove(run *caseRun) error { return fixtureApproval(run, true) }

func caseDeny(run *caseRun) error { return fixtureApproval(run, false) }

func fixtureApproval(run *caseRun, approve bool) error {
	if err := run.newSession(protocol.WorkModeCode, protocol.PermissionAsk); err != nil {
		return err
	}
	const path, original, replacement = "approval.txt", "SAILRY-UNCHANGED\n", "SAILRY-APPROVED\n"
	if err := run.seed(path, original); err != nil {
		return err
	}
	file, err := run.read(path)
	if err != nil {
		return err
	}
	if file.Revision == nil {
		return fail("Approval fixture has no revision")
	}
	turn, err := run.submit(fmt.Sprintf("Call write_file exactly once with path=%q, text=%q and expected_revision=%q. Wait for its approval. If denied, do not retry or use another tool; reply DENIED. If allowed, reply APPROVED. Do not call read_file or any other tool.", path, replacement, *file.Revision))
	if err != nil {
		return err
	}
	view, err := run.pending(turn.ID)
	if err != nil {
		return err
	}
	if len(view.approvals()) != 1 || len(view.questions()) != 0 {
		return blocked("The fixture did not produce exactly one expected write approval")
	}
	approval := view.approvals()[0]
	matched := false
	for _, call := range view.tools() {
		if call.reference.Entry != approval.Entry || call.reference.Index != approval.Index {
			continue
		}
		detail, err := run.detail(call)
		if err != nil {
			return err
		}
		// Only this exact revisioned fixture write is authorized for an
		// automatic answer. A different path or tool remains pending.
		if !approvalWriteMatches(call.name, detail, path, replacement, *file.Revision) {
			return blocked("Approval arguments differ from the exact fixture write; no answer was sent")
		}
		matched = true
	}
	if !matched || approval.Session != run.session.ID || approval.Turn != turn.ID {
		return blocked("Approval has no matching owned fixture tool; no answer was sent")
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	decision := "deny"
	if approve {
		decision = "approve"
	}
	if _, err := run.command(run.ctx, "resolve_approval", protocol.CommandResolveApprovalPayload{Session: run.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: decision}}); err != nil {
		return err
	}
	if run.resolved == nil {
		run.resolved = map[protocol.ApprovalID]bool{}
	}
	run.resolved[approval.ID] = true
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Model did not complete after the explicit fixture approval response")
	}
	state := protocol.ConversationApprovalStateDenied
	expected := original
	if approve {
		state, expected = protocol.ConversationApprovalStateApproved, replacement
	}
	if !slices.ContainsFunc(view.page.Approvals, func(value protocol.ConversationApproval) bool { return value.ID == approval.ID && value.State == state }) {
		return fail("Canonical approval did not retain the selected decision")
	}
	if err := run.fileEquals(path, expected); err != nil {
		return err
	}
	run.evidence("approval", "Exact fixture write was explicitly "+decision+"d; canonical decision and artifact inspected")
	return nil
}

func approvalWriteMatches(name string, detail protocol.ToolDetail, path, text, revision string) bool {
	return name == "write_file" && argumentString(detail, "path") == path && argumentString(detail, "text") == text && argumentString(detail, "expected_revision") == revision
}

func caseQuestionChoice(run *caseRun) error {
	return choiceQuestion(run, "SAILRY-CHOICE", []string{"Go", "Rust"}, 1)
}

func caseQuestionText(run *caseRun) error {
	return choiceQuestion(run, "SAILRY-CONFIRM", []string{"Yes", "No"}, 0)
}

func choiceQuestion(run *caseRun, prompt string, options []string, selected uint64) error {
	if err := run.standard(); err != nil {
		return err
	}
	encoded, _ := json.Marshal(options)
	turn, err := run.submit(fmt.Sprintf("Call ask_user exactly once with prompt=%q and input={kind:\"choice\",options:%s,multiple:false,allow_other:false}. Wait for the answer, then reply with exactly the selected option. Use no other tools.", prompt, encoded))
	if err != nil {
		return err
	}
	view, err := run.pending(turn.ID)
	if err != nil {
		return err
	}
	if len(view.questions()) != 1 || len(view.approvals()) != 0 {
		return blocked("The fixture did not produce exactly one expected choice question")
	}
	question := view.questions()[0]
	spec, err := run.questionSpec(view, question)
	if err != nil {
		return err
	}
	input, err := decode[protocol.ConversationQuestionInputChoicePayload](spec.Input.Data)
	if err != nil || spec.Prompt != prompt || spec.Input.Kind != "choice" || !slices.Equal(input.Options, options) || input.Multiple || input.AllowOther {
		return blocked("Question arguments differ from the controlled fixture; no answer was sent")
	}
	response := protocol.ConversationQuestionResponse{Kind: "answer", Data: protocol.ConversationQuestionAnswer{Kind: "choices", Data: protocol.ConversationQuestionAnswerChoicesPayload{Selected: []uint64{selected}}}}
	if err := run.answer(question, response); err != nil {
		return err
	}
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Model did not finish after the choice answer")
	}
	if err := assertQuestionState(view, question.ID, "answered"); err != nil {
		return err
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	return run.requireText(view, options[selected])
}

func caseQuestionDecline(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	turn, err := run.submit("Call ask_user exactly once with prompt=\"SAILRY-DECLINE\" and input={kind:\"text\",multiline:false,max_bytes:80}. If the user declines, reply exactly DECLINED without inventing an answer. Use no other tools.")
	if err != nil {
		return err
	}
	view, err := run.pending(turn.ID)
	if err != nil {
		return err
	}
	if len(view.questions()) != 1 || len(view.approvals()) != 0 {
		return blocked("The fixture did not produce one expected decline question")
	}
	question := view.questions()[0]
	spec, err := run.questionSpec(view, question)
	if err != nil {
		return err
	}
	if spec.Prompt != "SAILRY-DECLINE" || spec.Input.Kind != "text" {
		return blocked("Question arguments differ from the decline fixture; no response was sent")
	}
	// The ordinary question UI declines an answer with cancel. The distinct
	// decline response belongs only to MCP form and URL elicitation requests.
	if err := run.answer(question, protocol.ConversationQuestionResponse{Kind: "cancel"}); err != nil {
		return err
	}
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Model did not continue after the question was declined")
	}
	if err := assertQuestionState(view, question.ID, "cancelled"); err != nil {
		return err
	}
	return run.requireText(view, "DECLINED")
}

func assertQuestionState(view turnView, id protocol.QuestionID, kind string) error {
	if !slices.ContainsFunc(view.page.Questions, func(value protocol.ConversationQuestion) bool { return value.ID == id && value.State.Kind == kind }) {
		return fail("Canonical question did not retain its answered or declined state")
	}
	return nil
}
