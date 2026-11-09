package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// LockDecision claims the owner before a decision transaction can wait for
// COMMIT. Expiry uses the same owner, so it cannot overtake a claimed answer.
func (r *Runtime) LockDecision(command protocol.Command) func() {
	if command.Kind != "resolve_approval" && command.Kind != "resolve_question" {
		return func() {}
	}
	r.decisions.Lock()
	return r.decisions.Unlock
}
func (r *Runtime) deadline(ctx context.Context, duration time.Duration) (<-chan struct{}, context.CancelFunc) {
	timer, cancel := context.WithCancel(ctx)
	expired := make(chan struct{})
	go func() {
		if r.clock.Sleep(timer, duration) == nil {
			close(expired)
		}
	}()
	return expired, cancel
}
func toolIdentity(call canonical.ToolRequest) canonical.ToolIdentity {
	return canonical.ToolIdentity{ID: &call.ID, Name: call.Name, Arguments: call.Arguments}
}
func (r *Runtime) approval(x *execution, call canonical.ToolRequest, binding *store.ApprovalBinding) (bool, error) {
	var value protocol.ConversationApproval
	publication := r.LockPublication()
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		var changes []protocol.Event
		var err error
		value, changes, err = store.BeginApproval(x.ctx, db, x.inv.Run.Turn, toolIdentity(call), binding, r.clock.Now())
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(x.ctx, db, r.node, changes)
		return err
	})
	if err != nil {
		publication()
		return false, err
	}
	r.publish()
	r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "approval", Data: value})
	publication()
	expired, cancel := r.deadline(x.ctx, 300*time.Second)
	defer cancel()
	for value.State == protocol.ConversationApprovalStatePending {
		select {
		case <-x.ctx.Done():
			return false, x.ctx.Err()
		case <-x.wake:
			err = r.db.Read(x.ctx, func(db *sql.Conn) error {
				var err error
				value, err = store.ReadApproval(x.ctx, db, value.ID)
				return err
			})
		case <-expired:
			r.decisions.Lock()
			publication := r.LockPublication()
			err = r.db.Write(x.ctx, func(db *sql.Conn) error {
				var changes []protocol.Event
				var err error
				value, changes, err = store.ExpireApproval(x.ctx, db, value.ID)
				if err != nil {
					return err
				}
				_, err = store.RecordEvents(x.ctx, db, r.node, changes)
				return err
			})
			expired = nil
			if err == nil {
				r.publish()
				r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "approval", Data: value})
			}
			publication()
			r.decisions.Unlock()
		}
		if err != nil {
			return false, err
		}
	}
	if value.State != protocol.ConversationApprovalStateApproved {
		return false, nil
	}
	publication = r.LockPublication()
	defer publication()
	err = r.db.Write(x.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(x.ctx, "UPDATE agent_approvals SET claimed=1 WHERE id=?1 AND claimed=0 AND json_extract(body,'$.state')='approved'", value.ID.String())
		return err
	})
	return err == nil, err
}
func (r *Runtime) question(x *execution, call canonical.ToolRequest) ToolResult {
	var spec protocol.ConversationQuestionSpec
	if err := json.Unmarshal(call.Arguments, &spec); err != nil {
		return invalidTool("invalid question arguments")
	}
	var value protocol.ConversationQuestion
	publication := r.LockPublication()
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		var changes []protocol.Event
		var err error
		value, changes, err = store.BeginQuestion(x.ctx, db, x.inv.Run.Turn, toolIdentity(call))
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(x.ctx, db, r.node, changes)
		return err
	})
	if err != nil {
		publication()
		return toolFault(err)
	}
	r.publish()
	r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "question", Data: value})
	publication()
	var expired <-chan struct{}
	if spec.Input.Kind != "plan" {
		var cancel context.CancelFunc
		expired, cancel = r.deadline(x.ctx, 300*time.Second)
		defer cancel()
	}
	for value.State.Kind == "pending" {
		select {
		case <-x.ctx.Done():
			return toolFault(x.ctx.Err())
		case <-x.wake:
			err = r.db.Read(x.ctx, func(db *sql.Conn) error {
				var err error
				value, err = store.ReadQuestion(x.ctx, db, value.ID)
				return err
			})
		case <-expired:
			r.decisions.Lock()
			publication := r.LockPublication()
			err = r.db.Write(x.ctx, func(db *sql.Conn) error {
				var changes []protocol.Event
				var err error
				value, changes, err = store.ExpireQuestion(x.ctx, db, value.ID)
				if err != nil {
					return err
				}
				_, err = store.RecordEvents(x.ctx, db, r.node, changes)
				return err
			})
			expired = nil
			if err == nil {
				r.publish()
				r.feed.Publish(value.Session, protocol.ConversationChange{Kind: "question", Data: value})
			}
			publication()
			r.decisions.Unlock()
		}
		if err != nil {
			return toolFault(err)
		}
	}
	result := map[string]any{"status": value.State.Kind}
	if value.State.Kind == "timed_out" {
		result["source"] = "automatic_timeout"
		result["question"] = value.ID
		result["answers"] = []string{}
		result["recommendations"] = []any{}
		if spec.Input.Kind == "choice" {
			choice := spec.Input.Data.(*protocol.ConversationQuestionInputChoicePayload)
			result["recommendations"] = []any{map[string]any{"index": 0, "option": choice.Options[0]}}
		}
	} else if value.State.Kind == "answered" {
		answer := value.State.Data.(*protocol.ConversationQuestionAnswer)
		switch answer.Kind {
		case "text":
			result["answer"] = *answer.Data.(*string)
		case "choices":
			selection := answer.Data.(*protocol.ConversationQuestionAnswerChoicesPayload)
			options := spec.Input.Data.(*protocol.ConversationQuestionInputChoicePayload)
			answers := []string{}
			for _, i := range selection.Selected {
				answers = append(answers, options.Options[i])
			}
			if selection.Other != nil {
				answers = append(answers, *selection.Other)
			}
			result["answers"] = answers
		case "plan":
			result["status"] = "accepted"
			result["coding_turn"] = answer.Data.(*protocol.ConversationQuestionAnswerPlanPayload).Turn
		}
	}
	return ToolResult{Value: result}
}
