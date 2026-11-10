package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadQuestion(ctx context.Context, db *sql.Conn, id protocol.QuestionID) (protocol.ConversationQuestion, error) {
	v, e := readBody[protocol.ConversationQuestion](ctx, db, "SELECT body FROM agent_questions WHERE id=?1", id.String())
	if e != nil {
		return protocol.ConversationQuestion{}, e
	}
	if v == nil {
		return protocol.ConversationQuestion{}, fault(protocol.ErrorCodeNotFound, "question does not exist")
	}
	return *v, nil
}
func putQuestion(ctx context.Context, db *sql.Conn, value protocol.ConversationQuestion) error {
	body, err := encode(value)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE agent_questions SET body=?2 WHERE id=?1", value.ID.String(), body)
	return err
}
func validateQuestionSpec(spec protocol.ConversationQuestionSpec) error {
	if strings.TrimSpace(spec.Prompt) == "" && !(spec.Prompt == "" && spec.Input.Kind == "plan") {
		return invalid("question prompt is empty")
	}
	switch spec.Input.Kind {
	case "text":
		v := spec.Input.Data.(*protocol.ConversationQuestionInputTextPayload)
		if v.MaxBytes == 0 || v.MaxBytes > 64*1024 {
			return invalid("question text limit is invalid")
		}
	case "choice":
		v := spec.Input.Data.(*protocol.ConversationQuestionInputChoicePayload)
		if len(v.Options) == 0 {
			return invalid("question options are empty or repeated")
		}
		seen := map[string]bool{}
		for _, o := range v.Options {
			if strings.TrimSpace(o) == "" || seen[o] {
				return invalid("question options are empty or repeated")
			}
			seen[o] = true
		}
	case "plan":
	case "form", "url":
		return invalid("this input requires an MCP request")
	default:
		return invalid("invalid question input")
	}
	return nil
}
func BeginQuestion(ctx context.Context, db *sql.Conn, turn protocol.TurnID, identity canonical.ToolIdentity) (protocol.ConversationQuestion, []protocol.Event, error) {
	value := protocol.DefaultConversationQuestion()
	if identity.Name != "ask_user" && identity.Name != "plan_review" {
		return value, nil, invalid("tool does not accept user input")
	}
	var spec protocol.ConversationQuestionSpec
	if err := json.Unmarshal(identity.Arguments, &spec); err != nil {
		return value, nil, invalid("invalid question arguments")
	}
	if err := validateQuestionSpec(spec); err != nil {
		return value, nil, err
	}
	if identity.Name == "ask_user" && strings.TrimSpace(spec.Prompt) == "" {
		return value, nil, invalid("question prompt is empty")
	}
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return value, nil, err
	}
	if spec.Input.Kind == "plan" {
		capture, e := readTurnCapture(ctx, db, turn)
		if e != nil {
			return value, nil, e
		}
		if capture.Config.Mode != protocol.WorkModePlan {
			return value, nil, fault(protocol.ErrorCodePermissionDenied, "plan review requires a planning turn")
		}
	}
	entry, index, _, err := locateAgentCall(ctx, db, turn, identity)
	if err != nil {
		return value, nil, err
	}
	value.ID = protocol.NewQuestionID()
	value.Session = run.Session
	value.Turn = turn
	value.Entry = entry
	value.Index = index
	value.State = protocol.ConversationQuestionState{Kind: "pending"}
	body, err := encode(value)
	if err != nil {
		return value, nil, err
	}
	var exists bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND call=?2)", turn.String(), *identity.ID).Scan(&exists); err != nil {
		return value, nil, err
	}
	if exists {
		return value, nil, fault(protocol.ErrorCodeConflict, "tool call already requested user input")
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)", value.ID.String(), turn.String(), *identity.ID, body); err != nil {
		return value, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return value, events, err
}
func questionSpec(ctx context.Context, db *sql.Conn, value protocol.ConversationQuestion) (protocol.ConversationQuestionSpec, error) {
	var spec protocol.ConversationQuestionSpec
	record, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", value.Turn.String(), value.Entry)
	if err != nil {
		return spec, err
	}
	if record == nil {
		return spec, invalid("question canonical request is unavailable")
	}
	if review := planReviewSpec(*record); review != nil {
		return *review, nil
	}
	if input := elicitationInput(*record); input != nil {
		return input.Spec, nil
	}
	if message := record.Message(); message != nil {
		for _, block := range message.Content {
			if block.Index == value.Index {
				if call, ok := block.Data.(*canonical.ToolRequest); ok {
					err = json.Unmarshal(call.Arguments, &spec)
					return spec, err
				}
			}
		}
	}
	return spec, invalid("question canonical request is invalid")
}
func validateQuestionAnswer(spec protocol.ConversationQuestionSpec, answer protocol.ConversationQuestionAnswer) error {
	switch spec.Input.Kind {
	case "text":
		if answer.Kind != "text" {
			return invalid("question answer has another input kind")
		}
		text := *answer.Data.(*string)
		limit := spec.Input.Data.(*protocol.ConversationQuestionInputTextPayload)
		if strings.TrimSpace(text) == "" || uint64(len(text)) > limit.MaxBytes || !limit.Multiline && strings.ContainsAny(text, "\r\n") {
			return invalid("question text answer is invalid")
		}
	case "choice":
		if answer.Kind != "choices" {
			return invalid("question answer has another input kind")
		}
		value := answer.Data.(*protocol.ConversationQuestionAnswerChoicesPayload)
		input := spec.Input.Data.(*protocol.ConversationQuestionInputChoicePayload)
		if len(value.Selected) == 0 && (value.Other == nil || strings.TrimSpace(*value.Other) == "") {
			return invalid("question answer is empty")
		}
		if !input.Multiple && len(value.Selected)+boolCount(value.Other != nil) > 1 {
			return invalid("question permits one answer")
		}
		seen := map[uint64]bool{}
		for _, i := range value.Selected {
			if i >= uint64(len(input.Options)) || seen[i] {
				return invalid("question selection is invalid")
			}
			seen[i] = true
		}
		if value.Other != nil && (!input.AllowOther || strings.TrimSpace(*value.Other) == "" || len(*value.Other) > 64*1024) {
			return invalid("question other answer is invalid")
		}
	case "plan":
		if answer.Kind != "text" || len(*answer.Data.(*string)) > 64*1024 {
			return invalid("answer type does not match the question")
		}
	case "form":
		if answer.Kind != "form" {
			return invalid("answer type does not match the MCP form")
		}
	case "url":
		if answer.Kind != "opened" {
			return invalid("answer type does not match the MCP URL")
		}
	default:
		return invalid("question answer requires an MCP input")
	}
	return nil
}
func boolCount(v bool) int {
	if v {
		return 1
	}
	return 0
}
func resolveQuestion(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	input := request.Command.Data.(*protocol.CommandResolveQuestionPayload)
	value, err := ReadQuestion(ctx, db, input.Question)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if value.Session != input.Session {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeWrongTarget, "question belongs to another session")
	}
	spec, err := questionSpec(ctx, db, value)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if input.Response.Kind == "start_coding" {
		if value.State.Kind != "pending" {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "plan has already closed")
		}
		if spec.Input.Kind != "plan" {
			return protocol.Output{}, nil, invalid("question does not contain a plan")
		}
		if err = activeQuestionRun(ctx, db, value); err != nil {
			return protocol.Output{}, nil, err
		}
		response := input.Response.Data.(*protocol.ConversationQuestionResponseStartCodingPayload)
		session, e := checkedSession(ctx, db, input.Session, response.ExpectedRevision)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		if e = validateAgentInput(response.Message); e != nil {
			return protocol.Output{}, nil, e
		}
		events := []protocol.Event{}
		if session.Config.Mode != protocol.WorkModeCode {
			session.Config.Mode = protocol.WorkModeCode
			output, changed, e := reviseSession(ctx, db, session)
			if e != nil {
				return protocol.Output{}, nil, e
			}
			session = output.Data.(protocol.Session)
			events = append(events, changed...)
		}
		turn, e := admitAgentTurn(ctx, db, node, caller, request, session, protocol.ConversationRunKindTask, true)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		queue, e := ReadQueue(ctx, db, session.ID)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		var before *protocol.TurnID
		if len(queue.Items) > 0 {
			id := queue.Items[0].Turn
			before = &id
		}
		if e = reorderQueue(ctx, db, session.ID, turn.ID, before); e != nil {
			return protocol.Output{}, nil, e
		}
		answer := protocol.ConversationQuestionAnswer{Kind: "plan", Data: &protocol.ConversationQuestionAnswerPlanPayload{Turn: turn.ID}}
		value.State = protocol.ConversationQuestionState{Kind: "answered", Data: &answer}
		if e = putQuestion(ctx, db, value); e != nil {
			return protocol.Output{}, nil, e
		}
		session.Activity, e = readActivity(ctx, db, session.ID)
		if e != nil {
			return protocol.Output{}, nil, e
		}
		changed, e := activityEvent(ctx, db, session.ID)
		events = append(events, changed...)
		return protocol.Output{Kind: "plan_accepted", Data: protocol.ConversationQuestionAcceptedPlan{Question: value, Session: session, Turn: turn}}, events, e
	}
	state := protocol.ConversationQuestionState{}
	switch input.Response.Kind {
	case "cancel":
		state.Kind = "cancelled"
	case "decline":
		if !slices.Contains([]string{"form", "url"}, spec.Input.Kind) {
			return protocol.Output{}, nil, invalid("decline requires an MCP request")
		}
		state.Kind = "declined"
	case "answer":
		answer := input.Response.Data.(*protocol.ConversationQuestionAnswer)
		if err = validateQuestionAnswer(spec, *answer); err != nil {
			return protocol.Output{}, nil, err
		}
		if err = validateMCPAnswer(ctx, db, value, *answer); err != nil {
			return protocol.Output{}, nil, err
		}
		state.Kind = "answered"
		state.Data = answer
	default:
		return protocol.Output{}, nil, invalid("invalid question response")
	}
	if value.State.Kind != "pending" {
		a, _ := encode(value.State)
		b, _ := encode(state)
		if bytes.Equal(a, b) {
			return protocol.Output{Kind: "question", Data: value}, nil, nil
		}
		return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "question has already closed")
	}
	if err = activeQuestionRun(ctx, db, value); err != nil {
		return protocol.Output{}, nil, err
	}
	value.State = state
	if err = putQuestion(ctx, db, value); err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return protocol.Output{Kind: "question", Data: value}, events, err
}
func ExpireQuestion(ctx context.Context, db *sql.Conn, id protocol.QuestionID) (protocol.ConversationQuestion, []protocol.Event, error) {
	value, err := ReadQuestion(ctx, db, id)
	if err != nil {
		return value, nil, err
	}
	if value.State.Kind != "pending" {
		return value, nil, nil
	}
	spec, err := questionSpec(ctx, db, value)
	if err != nil {
		return value, nil, err
	}
	if spec.Input.Kind == "plan" {
		return value, nil, invalid("plan acceptance requires an explicit response")
	}
	value.State = protocol.ConversationQuestionState{Kind: "timed_out"}
	if err = putQuestion(ctx, db, value); err != nil {
		return value, nil, err
	}
	events, err := activityEvent(ctx, db, value.Session)
	return value, events, err
}

func activeQuestionRun(ctx context.Context, db *sql.Conn, value protocol.ConversationQuestion) error {
	completed, err := completedReview(ctx, db, value)
	if err != nil || completed {
		return err
	}
	_, err = activeAgentRun(ctx, db, value.Turn)
	return err
}
