package acceptance

import (
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func casePlan(run *caseRun) error {
	if err := run.newSession(protocol.WorkModePlan, protocol.PermissionProject); err != nil {
		return err
	}
	const text = "SAILRY-PLAN-READONLY\n"
	if err := run.seed("plan.txt", text); err != nil {
		return err
	}
	turn, err := run.submit("Read plan.txt using read_file, then provide a short plan containing PLAN-READY. This is planning mode: do not create, change or delete any file, run a command, or request coding mode.")
	if err != nil {
		return err
	}
	view, err := run.pending(turn.ID)
	if err != nil {
		return err
	}
	if err := run.assertToolPath(view, "read_file", "plan.txt"); err != nil {
		return err
	}
	if len(view.questions()) != 1 || len(view.approvals()) != 0 {
		return blocked("Planning did not reach its single review question")
	}
	question := view.questions()[0]
	review := false
	for _, call := range view.tools() {
		if call.name != "read_file" && call.name != "plan_review" {
			return fail("Planning fixture invoked a tool outside its declared read-only task")
		}
		if call.name == "plan_review" && call.reference.Entry == question.Entry && call.reference.Index == question.Index {
			detail, err := run.detail(call)
			if err != nil {
				return err
			}
			spec, err := decode[protocol.ConversationQuestionSpec](detail.Arguments)
			if err != nil || spec.Input.Kind != "plan" {
				return fail("The planning review does not contain a plan question")
			}
			review = true
		}
	}
	if !review {
		return fail("The planning question has no matching canonical review")
	}
	// Completing a plan publishes a review question in the same turn. Cancel
	// that review explicitly: it must neither enter coding mode nor run a tool.
	if err := run.answer(question, protocol.ConversationQuestionResponse{Kind: "cancel"}); err != nil {
		return err
	}
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("The read-only plan did not complete after its review was canceled")
	}
	if err := assertQuestionState(view, question.ID, "cancelled"); err != nil {
		return err
	}
	output, err := run.command(run.ctx, "read_turn_capture", protocol.CommandReadTurnCapturePayload{Session: run.session.ID, Turn: view.run.Turn})
	if err != nil {
		return err
	}
	capture, err := outputAs[protocol.QueuedTurn](output, "queued_turn")
	if err != nil {
		return err
	}
	if capture.Config.Mode != protocol.WorkModePlan {
		return fail("Planning mode was not captured with the turn")
	}
	if err := run.refreshSession(); err != nil {
		return err
	}
	if run.session.Config.Mode != protocol.WorkModePlan {
		return fail("Canceling plan review changed the session out of planning mode")
	}
	if err := run.fileEquals("plan.txt", text); err != nil {
		return err
	}
	run.evidence("scope", "Read-only planning behavior and captured mode; permission enforcement has deterministic Node tests")
	return run.requireText(view, "PLAN-READY")
}

func caseGuidance(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	// A question is a stable model boundary, avoiding a race against a short
	// free-text answer while still exercising actual turn guidance delivery.
	turn, err := run.submit("Call ask_user exactly once with prompt=\"SAILRY-GUIDANCE-GATE\" and input={kind:\"text\",multiline:false,max_bytes:80}. After its answer, follow any new guidance and reply briefly. Use no other tools.")
	if err != nil {
		return err
	}
	view, err := run.pending(turn.ID)
	if err != nil {
		return err
	}
	if len(view.questions()) != 1 || len(view.approvals()) != 0 {
		return blocked("Guidance fixture did not reach its expected question boundary")
	}
	question := view.questions()[0]
	spec, err := run.questionSpec(view, question)
	if err != nil {
		return err
	}
	if spec.Prompt != "SAILRY-GUIDANCE-GATE" || spec.Input.Kind != "text" {
		return blocked("Guidance gate differs from the controlled fixture; no response was sent")
	}
	if err := run.refreshSession(); err != nil {
		return err
	}
	output, err := run.command(run.ctx, "guide_turn", protocol.CommandGuideTurnPayload{Recipient: turn.ID, ExpectedRevision: run.session.Revision, Message: message("Reply exactly GUIDANCE-DELIVERED, without any more tools.")})
	if err != nil {
		return err
	}
	guidance, err := run.recordTurn(output, "queued_turn")
	if err != nil {
		return err
	}
	if err := run.answer(question, protocol.ConversationQuestionResponse{Kind: "answer", Data: protocol.ConversationQuestionAnswer{Kind: "text", Data: "Continue"}}); err != nil {
		return err
	}
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Guidance recipient did not complete")
	}
	if err := run.requireText(view, "GUIDANCE-DELIVERED"); err != nil {
		return err
	}
	joined, err := run.wait(guidance.ID, nil)
	if err != nil {
		return err
	}
	if joined.run.Status != protocol.ConversationStatusCompleted || joined.run.Guidance == nil || *joined.run.Guidance != turn.ID || joined.run.Joined == nil || *joined.run.Joined != turn.ID {
		return fail("Guidance was not canonically joined to its intended recipient")
	}
	run.evidence("guidance", guidance.ID.String()+" joined recipient "+turn.ID.String())
	return nil
}

func (run *caseRun) enqueue(text string) (protocol.QueuedTurn, error) {
	if err := run.refreshSession(); err != nil {
		return protocol.QueuedTurn{}, err
	}
	output, err := run.command(run.ctx, "queue_turn", protocol.CommandQueueTurnPayload{Session: run.session.ID, ExpectedRevision: run.session.Revision, Message: message(text)})
	if err != nil {
		return protocol.QueuedTurn{}, err
	}
	return run.recordTurn(output, "queued_turn")
}

func (run *caseRun) queued(turn protocol.TurnID) (protocol.ConversationQueuedMessage, error) {
	output, err := run.command(run.ctx, "read_queued_turn", protocol.CommandReadQueuedTurnPayload{Turn: turn})
	if err != nil {
		return protocol.ConversationQueuedMessage{}, err
	}
	return outputAs[protocol.ConversationQueuedMessage](output, "queued_message")
}

func caseQueue(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	first, err := run.enqueue("Without tools, reply exactly QUEUE-ORIGINAL.")
	if err != nil {
		return err
	}
	removed, err := run.enqueue("Without tools, reply exactly QUEUE-REMOVED.")
	if err != nil {
		return err
	}
	queued, err := run.queued(first.ID)
	if err != nil {
		return err
	}
	edited := message("Without tools, reply exactly QUEUE-EDITED.")
	if _, err := run.command(run.ctx, "edit_queued_turn", protocol.CommandEditQueuedTurnPayload{Turn: first.ID, ExpectedRevision: queued.Revision, Message: edited}); err != nil {
		return err
	}
	current, err := run.queued(first.ID)
	if err != nil {
		return err
	}
	if current.Revision <= queued.Revision || current.Message.Text != edited.Text {
		return fail("Queued edit did not retain its new message and revision")
	}
	toRemove, err := run.queued(removed.ID)
	if err != nil {
		return err
	}
	if _, err := run.command(run.ctx, "remove_queued_turn", protocol.CommandRemoveQueuedTurnPayload{Turn: removed.ID, ExpectedRevision: toRemove.Revision}); err != nil {
		return err
	}
	history, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return err
	}
	if len(history.Page.Queue.Items) != 1 || history.Page.Queue.Items[0].Turn != first.ID || history.Page.Queue.Items[0].Ready || slices.ContainsFunc(history.Page.Runs, func(value protocol.ConversationRun) bool { return value.Turn == removed.ID && active(value.Status) }) {
		return fail("Queue remove or held-input state differs from the canonical queue")
	}
	if _, err := run.command(run.ctx, "send_queued_turn", protocol.CommandSendQueuedTurnPayload{Turn: first.ID, ExpectedRevision: current.Revision}); err != nil {
		return err
	}
	view, err := run.wait(first.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted || strings.Contains(view.text(), "QUEUE-ORIGINAL") || strings.Contains(view.text(), "QUEUE-REMOVED") {
		return fail("Sent queued input did not complete solely with the edited content")
	}
	if err := run.assertCapture(first); err != nil {
		return err
	}
	run.evidence("queue", "Held input edited and inspected; another item removed; only the edited input sent")
	return run.requireText(view, "QUEUE-EDITED")
}

func caseCompactEmpty(run *caseRun) error {
	// The current Node always requests a model summary and requires a history
	// boundary. It has no empty-history no-op contract. Do not spend a paid
	// request or misreport that unsupported manual-checklist behavior as passed.
	return &caseIssue{"skipped", "Empty-history compaction has no no-op contract in the current Node; no model request was sent"}
}

func caseCompact(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	for index := range 3 {
		text := fmt.Sprintf("Remember the test-only secret-free token COMPACT-MEMORY-8624 and record %d. %s Reply only ACK, without tools.", index+1, strings.Repeat("Acceptance history fixture. ", 150))
		if _, err := run.ask(text); err != nil {
			return err
		}
	}
	if err := run.refreshSession(); err != nil {
		return err
	}
	focus := "Preserve the exact test-only token COMPACT-MEMORY-8624."
	output, err := run.command(run.ctx, "compact_context", protocol.CommandCompactContextPayload{Session: run.session.ID, ExpectedRevision: run.session.Revision, Focus: &focus})
	if err != nil {
		return err
	}
	turn, err := run.recordTurn(output, "queued_turn")
	if err != nil {
		return err
	}
	view, err := run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted || view.run.Kind != protocol.ConversationRunKindCompaction {
		return fail("Manual compaction did not complete as a compaction turn")
	}
	found := false
	for _, entry := range view.page.Entries {
		for _, part := range entry.Parts {
			if part.Kind == "compaction" {
				text, err := decode[string](part.Data)
				found = err == nil && strings.Contains(text, "COMPACT-MEMORY-8624")
			}
		}
	}
	if !found {
		return fail("Canonical compaction handoff did not retain the fixture token")
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	view, err = run.ask("Without tools, return only the test-only token from the earlier conversation records.")
	if err != nil {
		return err
	}
	run.evidence("compaction", "Canonical handoff inspected and prior context recalled in a new model turn")
	return run.requireText(view, "COMPACT-MEMORY-8624")
}
