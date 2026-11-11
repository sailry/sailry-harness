package engine

import (
	"context"
	"database/sql"
	"encoding/json"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) Elicit(ctx context.Context, turn protocol.TurnID, parent canonical.ToolRequest, request *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
	r.mu.Lock()
	owner := r.active[turn]
	r.mu.Unlock()
	if owner == nil || ctx.Err() != nil || owner.ctx.Err() != nil || request == nil || request.Params == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "MCP input requires an active tool call"}
	}
	// A question belongs to the conversation the user reads and answers. A
	// subagent has none, so its server's request is refused instead of waiting
	// on an answer nobody can give.
	if owner.inv.Session.Delegation != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "MCP input is unavailable to a subagent; the main conversation must run this tool"}
	}
	parameters := request.Params
	mode := parameters.Mode
	if mode == "" && parameters.URL != "" {
		mode = "url"
	}
	var spec protocol.ConversationQuestionSpec
	schema := json.RawMessage("null")
	switch mode {
	case "", "form":
		body, err := json.Marshal(parameters.RequestedSchema)
		if err != nil {
			return nil, invalidElicitation()
		}
		schema = body
		spec, err = store.MCPFormSpec(parameters.Message, schema)
		if err != nil {
			return nil, err
		}
	case "url":
		spec = protocol.ConversationQuestionSpec{Prompt: parameters.Message, Input: protocol.ConversationQuestionInput{Kind: "url", Data: &protocol.ConversationQuestionInputUrlPayload{Url: parameters.URL, ElicitationID: parameters.ElicitationID}}}
	default:
		return nil, invalidElicitation()
	}
	requestBody, err := json.Marshal(parameters)
	if err != nil {
		return nil, invalidElicitation()
	}
	record := canonical.NewRecord(turn, "notice", &canonical.Notice{Kind: "elicitation", Data: &canonical.Elicitation{
		Parent: toolIdentity(parent), Request: requestBody, Spec: spec, Schema: schema,
	}}, r.clock.Now())
	record.Provenance.Author = "mcp_elicitation"
	var value protocol.ConversationQuestion
	var entry protocol.ConversationEntry
	publication := r.LockPublication()
	err = r.db.Write(ctx, func(db *sql.Conn) error {
		var changes []protocol.Event
		var err error
		entry, value, changes, err = store.BeginMCPElicitation(ctx, db, record)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err != nil {
		publication()
		return nil, err
	}
	r.publish()
	r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "entry", Data: entry})
	r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "question", Data: value})
	publication()
	response := protocol.ConversationQuestionResponse{Kind: "cancel"}
	defer func() {
		if value.State.Kind == "pending" {
			r.cancelElicitation(owner, value.ID)
		}
	}()
	for value.State.Kind == "pending" {
		owner.inputMu.Lock()
		if owner.inputWake == nil {
			owner.inputWake = make(chan struct{})
		}
		wake := owner.inputWake
		owner.inputMu.Unlock()
		err = r.db.Read(ctx, func(db *sql.Conn) error {
			var err error
			value, err = store.ReadQuestion(ctx, db, value.ID)
			return err
		})
		if err != nil {
			return nil, err
		}
		if value.State.Kind != "pending" {
			break
		}
		select {
		case <-ctx.Done():
			return &sdk.ElicitResult{Action: "cancel"}, nil
		case <-owner.ctx.Done():
			return &sdk.ElicitResult{Action: "cancel"}, nil
		case <-wake:
		}
	}
	result := &sdk.ElicitResult{Action: "cancel"}
	switch value.State.Kind {
	case "answered":
		answer := value.State.Data.(*protocol.ConversationQuestionAnswer)
		response = protocol.ConversationQuestionResponse{Kind: "answer", Data: answer}
		result.Action = "accept"
		if answer.Kind == "form" {
			body, err := json.Marshal(answer.Data)
			if err != nil || json.Unmarshal(body, &result.Content) != nil {
				return nil, invalidElicitation()
			}
		} else if answer.Kind != "opened" {
			return nil, invalidElicitation()
		}
	case "declined":
		result.Action, response.Kind = "decline", "decline"
	}
	completed := canonical.NewRecord(turn, "notice", &canonical.Notice{Kind: "elicitation_result", Data: &canonical.ElicitationResult{Call: record.ID, Response: response}}, r.clock.Now())
	completed.Provenance.Author = "mcp_elicitation"
	if err := r.save(context.WithoutCancel(ctx), owner.inv.Session.ID, completed); err != nil {
		return nil, err
	}
	return result, nil
}

func (r *Runtime) cancelElicitation(owner *execution, id protocol.QuestionID) {
	ctx := context.WithoutCancel(owner.ctx)
	r.decisions.Lock()
	defer r.decisions.Unlock()
	unlock := r.LockPublication()
	defer unlock()
	var question protocol.ConversationQuestion
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		var changes []protocol.Event
		var err error
		question, changes, err = store.CancelMCPElicitation(ctx, db, id)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err == nil {
		r.publish()
		r.feed.Publish(question.Session, protocol.ConversationChange{Kind: "question", Data: question})
	}
}

func invalidElicitation() error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unsupported or invalid MCP input"}
}
