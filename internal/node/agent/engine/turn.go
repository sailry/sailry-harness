package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) execute(x *execution) {
	if r.hooksForTurn != nil {
		x.hooks = r.hooksForTurn(x.ctx, x.inv)
	}
	err := r.turn(x)
	r.drainOperations(x)
	status := protocol.ConversationStatusCompleted
	var failure *protocol.Fault
	if x.ctx.Err() != nil {
		status = protocol.ConversationStatusInterrupted
	} else if err != nil {
		status = protocol.ConversationStatusFailed
		failure = model.Fault(err)
	}
	// Final effects use the Node lifetime, independently of the stopped request.
	ctx := context.WithoutCancel(x.ctx)
	if status == protocol.ConversationStatusInterrupted {
		r.feed.Seal(x.inv.Session.ID, x.inv.Run.Turn)
		if state, e := r.load(ctx, x.inv.Session.ID); e == nil {
			calls := pending(state.Records)
			if len(calls) > 0 {
				blocks := make([]canonical.Block, 0, len(calls))
				for _, call := range calls {
					blocks = append(blocks, responseBlock(uint64(len(blocks)), call, ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call was interrupted before completing"}}))
				}
				record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, blocks...), r.clock.Now())
				record.Provenance.Author = "tool"
				if e = r.save(ctx, x.inv.Session.ID, record); e != nil {
					failure = model.Fault(e)
				}
			}
		}
	}
	hookStatus := "succeeded"
	if status != protocol.ConversationStatusCompleted {
		hookStatus = "failed"
	}
	if hookErr := x.hooks.Complete(ctx, hookStatus); hookErr != nil && status != protocol.ConversationStatusInterrupted {
		status = protocol.ConversationStatusFailed
		failure = model.Fault(hookErr)
	}
	var run protocol.ConversationRun
	var reviewEntry *protocol.ConversationEntry
	var reviewQuestion *protocol.ConversationQuestion
	unlock := r.LockPublication()
	defer unlock()
	err = r.db.Write(ctx, func(db *sql.Conn) error {
		var changes []protocol.Event
		var err error
		run, changes, err = store.FinishTurn(ctx, db, x.inv.Run.Turn, status, failure, r.clock.Now())
		if err != nil {
			return err
		}
		var reviewEvents []protocol.Event
		reviewEntry, reviewQuestion, reviewEvents, err = store.PublishPlanReview(ctx, db, x.inv.Run.Turn, r.clock.Now())
		if err != nil {
			return err
		}
		changes = append(changes, reviewEvents...)
		_, err = store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err == nil {
		r.publish()
		if reviewEntry != nil {
			r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "entry", Data: *reviewEntry})
		}
		if reviewQuestion != nil {
			r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "question", Data: *reviewQuestion})
		}
		r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
		r.RefreshCommitted(run.Session)
	}
}

func inputRecord(turn protocol.TurnID, input protocol.ConversationInput, nowClock *Runtime) canonical.Record {
	var blocks []canonical.Block
	if len(input.References) > 0 {
		raw, _ := json.Marshal(input.References)
		blocks = append(blocks, canonical.Text(0, "User-selected references (plugin and skill targets are explicit capability requests; attachment targets identify attachments of this message by id, and references sharing a label form one selection; other targets are context identifiers, not contents. Use read_file/list_directory for paths and read_session for conversations before describing them):\n"+string(raw)))
	}
	index := uint64(len(blocks))
	blocks = append(blocks, canonical.Text(index, input.Text))
	for _, id := range input.Attachments {
		mime := ""
		blocks = append(blocks, canonical.Block{Index: uint64(len(blocks)), Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &mime}}})
	}
	record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, blocks...), nowClock.clock.Now())
	record.Provenance.Author = "user"
	source := canonical.InputUser
	record.Metadata.Input = &canonical.Input{Turn: &turn, Source: &source, TextPart: &index, References: input.References}
	return record
}

func (r *Runtime) turn(x *execution) error {
	inv := x.inv
	if inv.AuthorizationFailure != nil {
		return inv.AuthorizationFailure
	}
	if inv.Provider == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "captured model provider is unavailable"}
	}
	binding, err := model.Capture(*inv.Provider, inv.Turn.Config, inv.Secret, inv.Session.ID)
	if err != nil {
		return err
	}
	languageModel, err := r.model(x.ctx, binding)
	if err != nil {
		return err
	}
	if err = r.captureTools(x); err != nil {
		return err
	}
	state, err := r.load(x.ctx, inv.Session.ID)
	if err != nil {
		return err
	}
	policy := prompt.Policy(inv.Turn.Config.Mode, inv.Turn.Config.Permission, inv.Session.Delegation != nil)
	environment := prompt.Environment(r.clock.Now(), inv.Turn.Config.Model, os.Getenv("SHELL"), inv.Worktree.Project != nil)
	personal, err := prompt.CapturePersonal(x.ctx, r.profile)
	if err != nil {
		return err
	}
	if inv.Worktree.Project == nil {
		if err = core.PrivateDirectory(inv.Worktree.Path); err != nil {
			return err
		}
	}
	guides, err := prompt.CaptureGuides(x.ctx, inv.Worktree.Path, nil)
	if err != nil {
		return err
	}
	var initial []canonical.Record
	if delta := prompt.Changes(state.Context, policy, environment, personal, guides); delta != nil {
		record := canonical.NewRecord(inv.Run.Turn, "configuration", &canonical.Configuration{Context: *delta}, r.clock.Now())
		record.Provenance.Author = "assistant"
		initial = append(initial, record)
	}
	if inv.Run.Kind != protocol.ConversationRunKindCompaction {
		initial = append(initial, inputRecord(inv.Run.Turn, inv.Message, r))
	}
	if err = r.save(x.ctx, inv.Session.ID, initial...); err != nil {
		return err
	}
	recoveries := 0
	for {
		if err = x.ctx.Err(); err != nil {
			return err
		}
		state, err = r.load(x.ctx, inv.Session.ID)
		if err != nil {
			return err
		}
		if inv.Run.Kind == protocol.ConversationRunKindCompaction {
			return r.compact(x, binding, state, true)
		}
		err = r.db.Read(x.ctx, func(db *sql.Conn) error {
			var err error
			state.Reviewed, err = store.AgentTurnReviewed(x.ctx, db, inv.Run.Turn)
			return err
		})
		if err != nil {
			return err
		}
		if state.Reviewed {
			return nil
		}
		// Each applicable operation commits before the next state reload.
		if calls := pending(state.Records); len(calls) > 0 {
			if err = r.runTools(x, state, calls); err != nil {
				return err
			}
			continue
		}
		joined, err := r.guidance(x)
		if err != nil {
			return err
		}
		if joined {
			continue
		}
		last := lastMessage(state.Records)
		if last != nil && last.Error != nil {
			if last.Error.Code != nil && *last.Error.Code == "context_overflow" && recoveries < 2 {
				if err = r.compact(x, binding, state, false); err != nil {
					return err
				}
				recoveries++
				continue
			}
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: last.Error.Message}
		}
		if shouldCompact(binding, state, inv.CompactionPercent) {
			if err = r.compact(x, binding, state, false); err != nil {
				return err
			}
			continue
		}
		if last != nil && last.Role == canonical.Assistant {
			return nil
		}
		if err = r.infer(x, binding, languageModel, state); err != nil {
			return err
		}
	}
}

func (r *Runtime) load(ctx context.Context, session protocol.SessionID) (store.AgentState, error) {
	var state store.AgentState
	err := r.db.Read(ctx, func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(ctx, db, session)
		if err != nil {
			return err
		}
		for i := range state.Records {
			if message := state.Records[i].MutableMessage(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok {
						value, err := store.HydrateCanonicalPayload(ctx, db, result.Result)
						if err != nil {
							return err
						}
						result.Result = canonical.Payload{Value: value}
					}
				}
			}
		}
		return nil
	})
	return state, err
}

func (r *Runtime) save(ctx context.Context, session protocol.SessionID, records ...canonical.Record) error {
	if len(records) == 0 {
		return nil
	}
	unlock := r.LockPublication()
	defer unlock()
	var entries []protocol.ConversationEntry
	var queue *protocol.ConversationQueue
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		entries = nil
		queue = nil
		var changes []protocol.Event
		input := false
		for _, record := range records {
			entry, events, err := store.AppendCanonical(ctx, db, session, record)
			if err != nil {
				return err
			}
			entries = append(entries, entry)
			changes = append(changes, events...)
			input = input || record.Provenance.Author == "user"
		}
		if input {
			current, err := store.ReadQueue(ctx, db, session)
			if err != nil {
				return err
			}
			queue = &current
		}
		_, err := store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err != nil {
		return err
	}
	r.publish()
	for i, entry := range entries {
		runtime := records[i].Metadata.Runtime
		if records[i].Kind != "compaction" && runtime != nil && !runtime.UserVisible {
			r.feed.Publish(session, protocol.ConversationChange{Kind: "hidden", Data: protocol.ConversationChangeHiddenPayload{Turn: entry.Turn, ID: entry.ID}})
		} else {
			r.feed.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
		}
	}
	if queue != nil {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "queue", Data: *queue})
	}
	r.RefreshStatisticsCommitted(session)
	return nil
}

func (r *Runtime) guidance(x *execution) (bool, error) {
	ctx := x.ctx
	unlock := r.LockPublication()
	defer unlock()
	var entries []protocol.ConversationEntry
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		values, changes, err := store.TakeGuidance(ctx, db, x.inv.Run.Turn)
		if err != nil {
			return err
		}
		for _, value := range values {
			record := inputRecord(value.Turn.ID, value.Message, r)
			runtime := canonical.DefaultRuntime()
			runtime.Steer = true
			record.Metadata.Runtime = &runtime
			entry, events, err := store.AppendCanonical(ctx, db, x.inv.Session.ID, record)
			if err != nil {
				return err
			}
			entries = append(entries, entry)
			changes = append(changes, events...)
		}
		_, err = store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err == nil && len(entries) > 0 {
		r.publish()
		for _, entry := range entries {
			r.feed.Publish(x.inv.Session.ID, protocol.ConversationChange{Kind: "entry", Data: entry})
		}
		r.RefreshCommitted(x.inv.Session.ID)
	}
	return len(entries) > 0, err
}

func lastMessage(records []canonical.Record) *canonical.Message {
	for i := len(records) - 1; i >= 0; i-- {
		if message := records[i].Message(); message != nil {
			if records[i].Metadata.Runtime == nil || records[i].Metadata.Runtime.AgentVisible || message.Error != nil {
				return message
			}
		}
	}
	return nil
}

func pending(records []canonical.Record) []canonical.ToolRequest {
	start := 0
	for i := range records {
		if m := records[i].Message(); m != nil && m.Role == canonical.User && records[i].Provenance.Author == "user" {
			start = i
		}
	}
	answered := map[string]bool{}
	for i := start; i < len(records); i++ {
		if m := records[i].Message(); m != nil {
			for _, block := range m.Content {
				if v, ok := block.Data.(*canonical.ToolResponse); ok {
					answered[v.ID] = true
				}
			}
		}
	}
	var calls []canonical.ToolRequest
	for i := start; i < len(records); i++ {
		if m := records[i].Message(); m != nil {
			for _, block := range m.Content {
				if v, ok := block.Data.(*canonical.ToolRequest); ok && !answered[v.ID] {
					var meta map[string]any
					_ = json.Unmarshal(v.ToolMeta, &meta)
					if meta[canonical.ExternalDispatchKey] != true {
						calls = append(calls, *v)
						answered[v.ID] = true
					}
				}
			}
		}
	}
	return calls
}
