package acceptance

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type turnView struct {
	page    protocol.ConversationPage
	run     protocol.ConversationRun
	drafts  []protocol.ConversationDraft
	aliases map[string]string
}

type toolCall struct {
	name      string
	summary   *protocol.ToolDetailSummary
	reference protocol.ToolDetailReference
}

func active(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}

func (view turnView) text() string {
	var output strings.Builder
	for _, entry := range view.page.Entries {
		if entry.Turn != view.run.Turn || entry.Author == "user" {
			continue
		}
		for _, part := range entry.Parts {
			if part.Kind == "text" {
				value, err := decode[string](part.Data)
				if err == nil {
					output.WriteString(value)
				}
			}
		}
	}
	return output.String()
}

func (view turnView) hasDraft() bool {
	for _, draft := range view.drafts {
		if draft.Turn == view.run.Turn && len(draft.Parts) > 0 {
			return true
		}
	}
	return view.text() != ""
}

func (view turnView) tools() []toolCall {
	values := []toolCall{}
	type identity struct {
		branch string
		call   string
	}
	pending := map[identity][]int{}
	for _, entry := range view.page.Entries {
		if entry.Turn != view.run.Turn {
			continue
		}
		for index, part := range entry.Parts {
			switch part.Kind {
			case "tool_call":
				value, err := decode[protocol.ConversationPartToolCallPayload](part.Data)
				if err != nil {
					continue
				}
				ref := protocol.ToolDetailReference{Session: view.page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: value.ID}
				if value.Summary != nil {
					ref.Revision = value.Summary.Revision
				}
				if value.ID != nil {
					key := identity{entry.Branch, *value.ID}
					pending[key] = append(pending[key], len(values))
				}
				values = append(values, toolCall{value.Name, value.Summary, ref})
			case "tool_result":
				value, err := decode[protocol.ConversationPartToolResultPayload](part.Data)
				if err != nil || value.ID == nil {
					continue
				}
				key := identity{entry.Branch, *value.ID}
				indices := pending[key]
				delete(pending, key)
				if len(indices) != 1 || values[indices[0]].name != value.Name {
					continue
				}
				call := &values[indices[0]]
				call.summary = value.Summary
				call.reference = protocol.ToolDetailReference{Session: view.page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(index), Call: value.ID}
				if value.Summary != nil {
					call.reference.Revision = value.Summary.Revision
				}
			}
		}
	}
	for index := range values {
		if declared := view.aliases[values[index].name]; declared != "" {
			values[index].name = declared
		}
	}
	return values
}

func (view turnView) approvals() []protocol.ConversationApproval {
	values := []protocol.ConversationApproval{}
	for _, approval := range view.page.Approvals {
		if approval.Turn == view.run.Turn && approval.State == protocol.ConversationApprovalStatePending {
			values = append(values, approval)
		}
	}
	return values
}

func (view turnView) questions() []protocol.ConversationQuestion {
	values := []protocol.ConversationQuestion{}
	for _, question := range view.page.Questions {
		if question.Turn == view.run.Turn && question.State.Kind == "pending" {
			values = append(values, question)
		}
	}
	return values
}

func (run *caseRun) wait(turn protocol.TurnID, condition func(turnView) (bool, error)) (turnView, error) {
	return run.observe(turn, condition, false)
}

func (run *caseRun) observe(turn protocol.TurnID, condition func(turnView) (bool, error), allowPending bool) (turnView, error) {
	for {
		if err := run.ctx.Err(); err != nil {
			return turnView{}, err
		}
		state, known := run.controller.Conversation(run.session.ID)
		if known && state.Err != nil {
			return turnView{}, state.Err
		}
		if known && state.OlderErr != nil {
			return turnView{}, state.OlderErr
		}
		if known && state.Removed {
			return turnView{}, fail("Acceptance session was removed while observed")
		}
		if known && state.Connected && !state.Loading {
			page := state.Snapshot.Page
			index := slices.IndexFunc(page.Runs, func(value protocol.ConversationRun) bool { return value.Turn == turn })
			if index >= 0 && !slices.Contains(state.Snapshot.Missing, turn) {
				view := turnView{page: page, run: page.Runs[index], drafts: state.Snapshot.Drafts}
				view, err := run.resolveToolNames(view)
				if err != nil {
					return view, err
				}
				if !allowPending && run.unhandled(view) {
					return view, blocked("Unexpected approval or question; no automatic response was sent")
				}
				if condition != nil {
					done, err := condition(view)
					if done || err != nil {
						return view, err
					}
				}
				if !active(view.run.Status) {
					durable, sequence, err := run.durable(turn)
					if err != nil {
						return view, err
					}
					if state.Snapshot.Sequence >= sequence {
						run.usage(durable)
						return durable, nil
					}
					if err := run.controller.RefreshConversation(run.ctx, run.session.ID); err != nil {
						return view, err
					}
				}
			} else if page.NextBefore != nil || len(state.Snapshot.Missing) > 0 {
				if err := run.controller.LoadOlderConversation(run.ctx, run.session.ID); err != nil {
					return turnView{}, err
				}
			}
		}
		select {
		case <-run.ctx.Done():
			return turnView{}, run.ctx.Err()
		case <-run.wake:
		}
	}
}

func (run *caseRun) durable(turn protocol.TurnID) (turnView, uint64, error) {
	history, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return turnView{}, 0, err
	}
	var before *uint64
	view := turnView{page: history.Page}
	view.page.Entries = nil
	for {
		output, err := run.command(run.ctx, "read_turn", protocol.CommandReadTurnPayload{Session: run.session.ID, Turn: turn, ExpectedRevision: history.Page.Revision, Before: before, Limit: 500})
		if err != nil {
			return view, 0, err
		}
		part, err := outputAs[protocol.ConversationTurnHistory](output, "turn_history")
		if err != nil {
			return view, 0, err
		}
		view.run = part.Run
		view.page.Entries = append(part.Entries, view.page.Entries...)
		view.page.Approvals, view.page.Questions = part.Approvals, part.Questions
		if part.NextBefore == nil {
			view, err := run.resolveToolNames(view)
			return view, part.Sequence, err
		}
		if before != nil && *part.NextBefore >= *before {
			return view, 0, fail("Turn history pagination did not advance")
		}
		before = part.NextBefore
	}
}

func (run *caseRun) usage(view turnView) {
	var total protocol.ConversationUsage
	known := false
	for _, entry := range view.page.Entries {
		if entry.Turn == view.run.Turn && entry.Usage != nil {
			known = true
			total.Input += entry.Usage.Input
			total.Output += entry.Usage.Output
			total.CachedInput += entry.Usage.CachedInput
			total.Reasoning += entry.Usage.Reasoning
		}
	}
	if known {
		run.evidence("usage", fmt.Sprintf("turn=%s input=%d output=%d cached=%d reasoning=%d", view.run.Turn.String(), total.Input, total.Output, total.CachedInput, total.Reasoning))
	} else {
		run.evidence("usage", "turn="+view.run.Turn.String()+" provider usage unavailable")
	}
}

func (run *caseRun) pending(turn protocol.TurnID) (turnView, error) {
	return run.observe(turn, func(view turnView) (bool, error) {
		return len(view.approvals()) > 0 || len(view.questions()) > 0, nil
	}, true)
}

func (run *caseRun) answer(question protocol.ConversationQuestion, response protocol.ConversationQuestionResponse) error {
	_, err := run.command(run.ctx, "resolve_question", protocol.CommandResolveQuestionPayload{Session: run.session.ID, Question: question.ID, Response: response})
	if err == nil {
		if run.answered == nil {
			run.answered = map[protocol.QuestionID]bool{}
		}
		run.answered[question.ID] = true
	}
	return err
}

func (run *caseRun) unhandled(view turnView) bool {
	for _, approval := range view.approvals() {
		if !run.resolved[approval.ID] {
			return true
		}
	}
	for _, question := range view.questions() {
		if !run.answered[question.ID] {
			return true
		}
	}
	return false
}

func (run *caseRun) questionSpec(view turnView, question protocol.ConversationQuestion) (protocol.ConversationQuestionSpec, error) {
	for _, call := range view.tools() {
		if call.reference.Entry == question.Entry && call.reference.Index == question.Index && call.name == "ask_user" {
			detail, err := run.detail(call)
			if err != nil {
				return protocol.ConversationQuestionSpec{}, err
			}
			var spec protocol.ConversationQuestionSpec
			if err := json.Unmarshal(detail.Arguments, &spec); err != nil {
				return spec, fail("Question tool arguments are invalid")
			}
			return spec, nil
		}
	}
	return protocol.ConversationQuestionSpec{}, fail("Pending question has no matching ask_user call")
}
