package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func planNotice(record canonical.Record) *canonical.Notice {
	if record.Kind != "notice" {
		return nil
	}
	switch value := record.Data.(type) {
	case *canonical.Notice:
		return value
	case canonical.Notice:
		return &value
	}
	return nil
}
func planReviewSpec(record canonical.Record) *protocol.ConversationQuestionSpec {
	if notice := planNotice(record); notice != nil && notice.Kind == "plan_review" {
		value := protocol.ConversationQuestionSpec{Prompt: "", Input: protocol.ConversationQuestionInput{Kind: "plan"}}
		return &value
	}
	return nil
}
func validatePlanReview(ctx context.Context, db *sql.Conn, record canonical.Record) error {
	notice := planNotice(record)
	if notice == nil || notice.Kind != "plan_review" {
		return nil
	}
	data, ok := notice.Data.(*canonical.PlanReview)
	if !ok {
		return invalid("plan review source is invalid")
	}
	run, err := ReadAgentRun(ctx, db, record.Turn)
	if err != nil {
		return err
	}
	capture, err := readTurnCapture(ctx, db, record.Turn)
	if err != nil {
		return err
	}
	if record.Provenance.Author != "plan_review" || run.Kind != protocol.ConversationRunKindTask || (run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusCompleted) || capture.Config.Mode != protocol.WorkModePlan {
		return invalid("plan review requires a planning completion")
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil {
		return err
	}
	if session.Delegation != nil {
		return invalid("child plans do not require review")
	}
	source, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", record.Turn.String(), data.Source)
	if err != nil {
		return err
	}
	if source == nil {
		return invalid("plan review has no completed answer")
	}
	message := source.Message()
	if message == nil || message.Error != nil || message.Role != canonical.Assistant || message.Finish != nil && (*message.Finish == canonical.Interrupted || *message.Finish == canonical.Failed) {
		return invalid("plan review has no completed answer")
	}
	for _, block := range message.Content {
		if block.Kind == "tool_request" {
			return invalid("plan review has no completed answer")
		}
	}
	return nil
}
func completedReview(ctx context.Context, db *sql.Conn, question protocol.ConversationQuestion) (bool, error) {
	if question.Index != 0 {
		return false, nil
	}
	run, err := ReadAgentRun(ctx, db, question.Turn)
	if err != nil {
		return false, err
	}
	if run.Session != question.Session || run.Kind != protocol.ConversationRunKindTask || run.Status != protocol.ConversationStatusCompleted || run.Error != nil {
		return false, nil
	}
	record, err := readBody[canonical.Record](ctx, db, "SELECT e.body FROM agent_events e JOIN conversation_events h ON h.sequence=e.sequence WHERE h.session=?1 AND e.turn=?2 AND e.id=?3", question.Session.String(), question.Turn.String(), question.Entry)
	if err != nil {
		return false, err
	}
	if record == nil || planReviewSpec(*record) == nil {
		return false, nil
	}
	return true, validatePlanReview(ctx, db, *record)
}
func PublishPlanReview(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (*protocol.ConversationEntry, *protocol.ConversationQuestion, []protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, nil, nil, err
	}
	if run.Status != protocol.ConversationStatusCompleted || run.Error != nil || run.Kind != protocol.ConversationRunKindTask {
		return nil, nil, nil, nil
	}
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return nil, nil, nil, err
	}
	if capture.Config.Mode != protocol.WorkModePlan {
		return nil, nil, nil, nil
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil {
		return nil, nil, nil, err
	}
	if session.Delegation != nil {
		return nil, nil, nil, nil
	}
	records, err := readBodies[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence", turn.String())
	if err != nil {
		return nil, nil, nil, err
	}
	var source *canonical.Record
	for i := range records {
		if planReviewSpec(records[i]) != nil {
			return nil, nil, nil, nil
		}
		if message := records[i].Message(); message != nil {
			// The reviewed answer is the last assistant text the user can see;
			// hidden runtime notes and empty messages after it do not hide it.
			if runtime := records[i].Metadata.Runtime; message.Role == canonical.Assistant && (runtime == nil || runtime.UserVisible) {
				for _, block := range message.Content {
					if text, ok := block.Data.(*canonical.TextContent); ok && text.Text != "" {
						source = &records[i]
						break
					}
				}
			}
			for _, block := range message.Content {
				if request, ok := block.Data.(*canonical.ToolRequest); ok && request.Name == "ask_user" && request.Error == nil {
					var spec protocol.ConversationQuestionSpec
					if json.Unmarshal(request.Arguments, &spec) == nil && spec.Input.Kind == "plan" && spec.Prompt != "" && validateQuestionSpec(spec) == nil {
						question, e := readBody[protocol.ConversationQuestion](ctx, db, "SELECT body FROM agent_questions WHERE turn=?1 AND call=?2", turn.String(), request.ID)
						if e != nil {
							return nil, nil, nil, e
						}
						if question == nil || !planFeedback(*question) {
							return nil, nil, nil, nil
						}
					}
				}
			}
		}
	}
	if source == nil {
		return nil, nil, nil, nil
	}
	message := source.Message()
	if message.Role != canonical.Assistant || message.Error != nil || message.Finish != nil && (*message.Finish == canonical.Interrupted || *message.Finish == canonical.Failed) {
		return nil, nil, nil, nil
	}
	for _, block := range message.Content {
		if block.Kind == "tool_request" {
			return nil, nil, nil, nil
		}
	}
	record := canonical.NewRecord(turn, "notice", &canonical.Notice{Kind: "plan_review", Data: &canonical.PlanReview{Source: source.ID}}, now)
	record.Provenance.Author = "plan_review"
	if err = validatePlanReview(ctx, db, record); err != nil {
		return nil, nil, nil, err
	}
	entry, _, err := AppendCanonical(ctx, db, run.Session, record)
	if err != nil {
		return nil, nil, nil, err
	}
	parts, err := reviewParts(record)
	if err != nil {
		return nil, nil, nil, err
	}
	entry.Parts = parts
	question := protocol.DefaultConversationQuestion()
	question.ID = protocol.NewQuestionID()
	question.Session = run.Session
	question.Turn = turn
	question.Entry = record.ID
	question.Index = 0
	question.State = protocol.ConversationQuestionState{Kind: "pending"}
	body, err := encode(question)
	if err != nil {
		return nil, nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)", question.ID.String(), turn.String(), record.ID, body); err != nil {
		return nil, nil, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return &entry, &question, events, err
}
func reviewRaw(record canonical.Record, index uint64) (*rawAgentTool, error) {
	if index != 0 {
		return nil, nil
	}
	if raw, err := elicitationRaw(record, index); raw != nil || err != nil {
		return raw, err
	}
	notice := planNotice(record)
	if notice == nil {
		return nil, nil
	}
	raw := &rawAgentTool{Arguments: json.RawMessage("null"), Name: "plan_review", Presentation: protocol.ToolPresentationSummary, Grouping: protocol.ToolGroupingSequence, Images: []protocol.ConversationImage{}}
	if spec := planReviewSpec(record); spec != nil {
		arguments, err := encode(spec)
		if err != nil {
			return nil, err
		}
		raw.ID = &record.ID
		raw.Arguments = arguments
		return raw, nil
	}
	if notice.Kind != "plan_result" {
		return nil, nil
	}
	response, ok := notice.Data.(*canonical.PlanResult)
	if !ok {
		return nil, invalid("plan review result is invalid")
	}
	var result map[string]any
	switch response.Response.Kind {
	case "answer":
		answer := response.Response.Data.(*protocol.ConversationQuestionAnswer)
		switch answer.Kind {
		case "text":
			result = map[string]any{"status": "answered", "answer": *answer.Data.(*string)}
		case "plan":
			result = map[string]any{"status": "accepted", "coding_turn": answer.Data.(*protocol.ConversationQuestionAnswerPlanPayload).Turn}
		default:
			return nil, invalid("plan review has an invalid response")
		}
	case "cancel":
		result = map[string]any{"status": "cancelled"}
	default:
		return nil, invalid("plan review has an invalid response")
	}
	raw.ID = &response.Call
	body, err := encode(result)
	if err != nil {
		return nil, err
	}
	data := json.RawMessage(body)
	raw.Result = &data
	return raw, nil
}
func reviewParts(record canonical.Record) ([]protocol.ConversationPart, error) {
	raw, err := reviewRaw(record, 0)
	if err != nil {
		return nil, err
	}
	if raw == nil {
		return nil, nil
	}
	part, err := raw.part()
	if err != nil {
		return nil, err
	}
	return []protocol.ConversationPart{part}, nil
}

type AgentReviewChange struct {
	Session protocol.SessionID
	Change  protocol.ConversationChange
}

func resolveReviewRecord(ctx context.Context, db *sql.Conn, question protocol.ConversationQuestion, now time.Time) ([]AgentReviewChange, []protocol.Event, error) {
	completed, err := completedReview(ctx, db, question)
	if err != nil || !completed {
		return nil, nil, err
	}
	var response protocol.ConversationQuestionResponse
	switch question.State.Kind {
	case "answered":
		response = protocol.ConversationQuestionResponse{Kind: "answer", Data: question.State.Data.(*protocol.ConversationQuestionAnswer)}
	case "cancelled":
		response = protocol.ConversationQuestionResponse{Kind: "cancel"}
	default:
		return nil, nil, invalid("completion review is not resolved")
	}
	id := "plan-review-result:" + question.Entry
	previous, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", question.Turn.String(), id)
	if err != nil {
		return nil, nil, err
	}
	if previous != nil {
		notice := planNotice(*previous)
		if notice == nil || notice.Kind != "plan_result" || previous.Provenance.Author != "plan_review" {
			return nil, nil, invalid("review result metadata is missing")
		}
		data, ok := notice.Data.(*canonical.PlanResult)
		a, _ := encode(response)
		var b []byte
		if ok {
			b, _ = encode(data.Response)
		}
		if !ok || data.Call != question.Entry || !bytes.Equal(a, b) {
			return nil, nil, fault(protocol.ErrorCodeConflict, "review result already differs")
		}
		return nil, nil, nil
	}
	record := canonical.NewRecord(question.Turn, "notice", &canonical.Notice{Kind: "plan_result", Data: &canonical.PlanResult{Call: question.Entry, Response: response}}, now)
	record.ID = id
	record.Provenance.Author = "plan_review"
	entry, events, err := AppendCanonical(ctx, db, question.Session, record)
	if err != nil {
		return nil, nil, err
	}
	parts, err := reviewParts(record)
	if err != nil {
		return nil, nil, err
	}
	entry.Parts = parts
	return []AgentReviewChange{{Session: question.Session, Change: protocol.ConversationChange{Kind: "entry", Data: entry}}}, events, nil
}

// ApplyReviewCommand runs inside the command transaction and returns feed effects
// to publish after COMMIT. It never owns another command or event ledger.
func ApplyReviewCommand(ctx context.Context, db *sql.Conn, request protocol.Request, output protocol.Output, now time.Time) ([]AgentReviewChange, []protocol.Event, error) {
	switch output.Kind {
	case "queued_turn":
		if request.Command.Kind != "submit_turn" && request.Command.Kind != "queue_turn" && request.Command.Kind != "submit_task" {
			return nil, nil, nil
		}
		var turn protocol.QueuedTurn
		switch value := output.Data.(type) {
		case protocol.QueuedTurn:
			turn = value
		case *protocol.QueuedTurn:
			turn = *value
		default:
			return nil, nil, invalid("queued turn result is invalid")
		}
		questions, err := readBodies[protocol.ConversationQuestion](ctx, db, "SELECT q.body FROM agent_questions q JOIN conversation_turns h ON h.turn=q.turn WHERE h.session=?1 AND json_extract(q.body,'$.state.kind')='pending'", turn.Session.String())
		if err != nil {
			return nil, nil, err
		}
		changes := []AgentReviewChange{}
		events := []protocol.Event{}
		for _, question := range questions {
			if question.Session != turn.Session {
				continue
			}
			completed, e := completedReview(ctx, db, question)
			if e != nil {
				return nil, nil, e
			}
			if !completed {
				continue
			}
			question.State = protocol.ConversationQuestionState{Kind: "cancelled"}
			if e = putQuestion(ctx, db, question); e != nil {
				return nil, nil, e
			}
			added, changed, e := resolveReviewRecord(ctx, db, question, now)
			if e != nil {
				return nil, nil, e
			}
			changes = append(changes, added...)
			events = append(events, changed...)
			changes = append(changes, AgentReviewChange{Session: question.Session, Change: protocol.ConversationChange{Kind: "question", Data: question}})
		}
		return changes, events, nil
	case "question", "plan_accepted":
		if request.Command.Kind != "resolve_question" {
			return nil, nil, nil
		}
		var question protocol.ConversationQuestion
		switch value := output.Data.(type) {
		case protocol.ConversationQuestion:
			question = value
		case *protocol.ConversationQuestion:
			question = *value
		case protocol.ConversationQuestionAcceptedPlan:
			question = value.Question
		case *protocol.ConversationQuestionAcceptedPlan:
			question = value.Question
		default:
			return nil, nil, invalid("question result is invalid")
		}
		return resolveReviewRecord(ctx, db, question, now)
	}
	return nil, nil, nil
}

func planFeedback(question protocol.ConversationQuestion) bool {
	answer, ok := question.State.Data.(*protocol.ConversationQuestionAnswer)
	if question.State.Kind != "answered" || !ok || answer.Kind != "text" {
		return false
	}
	return strings.TrimSpace(*answer.Data.(*string)) != ""
}

// AgentTurnReviewed follows explicit completion of a planning input. Nonempty
// feedback returns through the tool response so the model can revise its plan.
func AgentTurnReviewed(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (bool, error) {
	questions, err := readBodies[protocol.ConversationQuestion](ctx, db, "SELECT body FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='answered'", turn.String())
	if err != nil {
		return false, err
	}
	for _, question := range questions {
		answer, ok := question.State.Data.(*protocol.ConversationQuestionAnswer)
		if !ok || answer.Kind != "text" && answer.Kind != "plan" {
			continue
		}
		spec, err := questionSpec(ctx, db, question)
		if err != nil {
			return false, err
		}
		if spec.Input.Kind == "plan" && !planFeedback(question) {
			return true, nil
		}
	}
	return false, nil
}
