package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"log/slog"
	"os"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// settlementRetry spaces repeated attempts to make a turn's end durable.
const settlementRetry = time.Second

// stoppedStatus names the end of a turn whose context was cancelled. Only a
// Node that is shutting down interrupts; every deliberate stop, of the turn
// itself or of a child through its parent, is cancelled.
func (r *Runtime) stoppedStatus() protocol.ConversationStatus {
	if r.ctx.Err() != nil {
		return protocol.ConversationStatusInterrupted
	}
	return protocol.ConversationStatusCancelled
}

func (r *Runtime) execute(x *execution) {
	if r.working != nil {
		defer r.working()()
	}
	x.timeline = newTimeline(r.clock)
	if r.hooksForTurn != nil {
		x.hooks = r.hooksForTurn(x.ctx, x.inv)
	}
	err := r.recoveredTurn(x)
	r.drainOperations(x)
	// Final effects use the Node lifetime, independently of the stopped request.
	ctx := context.WithoutCancel(x.ctx)
	// A stopped or failed turn settles its unanswered calls before the
	// foreground cancellation notes, in the source order.
	var settleErr error
	if err != nil || x.ctx.Err() != nil {
		settleErr = r.settleCalls(ctx, x)
	}
	r.cancelForeground(x)
	cleanupErr := r.closeTools(x)
	status := protocol.ConversationStatusCompleted
	var failure *protocol.Fault
	if x.ctx.Err() != nil {
		status = r.stoppedStatus()
	} else if err != nil {
		status = protocol.ConversationStatusFailed
		failure = model.Fault(err)
	}
	if cleanupErr != nil {
		status = protocol.ConversationStatusFailed
		failure = model.Fault(cleanupErr)
	}
	if status == protocol.ConversationStatusCancelled || status == protocol.ConversationStatusInterrupted {
		r.feed.Seal(x.inv.Session.ID, x.inv.Run.Turn)
		if settleErr != nil {
			failure = model.Fault(settleErr)
		}
	}
	// Completion hooks belong to turns that ran to their end. A stopped turn
	// and a stopping Node run none.
	if x.ctx.Err() == nil {
		// The Turn End status is the turn's: completed or failed. After Tool
		// hooks report a call as succeeded or failed.
		hookStatus := "completed"
		if status != protocol.ConversationStatusCompleted {
			hookStatus = "failed"
		}
		if hookErr := x.hooks.Complete(ctx, hookStatus); hookErr != nil {
			status = protocol.ConversationStatusFailed
			failure = model.Fault(hookErr)
		}
	}
	// The turn's phases are stored once its work has ended, before its end.
	if phasesErr := r.save(ctx, x.inv.Session.ID, r.phasesRecord(x)); phasesErr != nil && status == protocol.ConversationStatusCompleted {
		status = protocol.ConversationStatusFailed
		failure = model.Fault(phasesErr)
	}
	r.mu.Lock()
	child := r.children[x.inv.Run.Turn]
	r.mu.Unlock()
	var run protocol.ConversationRun
	var successor *protocol.ConversationRun
	var reviewEntry *protocol.ConversationEntry
	var reviewQuestion *protocol.ConversationQuestion
	// The turn keeps its owner until its end is durable. Only this write is
	// repeated: the model, tools, hooks and cleanup have already ended.
	reported := false
	var unlock func()
	defer func() {
		if unlock != nil {
			unlock()
		}
	}()
	for {
		unlock = r.LockPublication()
		err = r.db.Write(ctx, func(db *sql.Conn) error {
			successor, reviewEntry, reviewQuestion = nil, nil, nil
			var changes []protocol.Event
			var err error
			if status == protocol.ConversationStatusCompleted && x.ctx.Err() != nil {
				status = r.stoppedStatus()
			}
			run, changes, err = store.FinishTurn(ctx, db, x.inv.Run.Turn, status, failure, r.clock.Now())
			if err != nil {
				return err
			}
			if child != nil && run.Status == protocol.ConversationStatusCompleted {
				var guidanceEvents []protocol.Event
				successor, guidanceEvents, err = store.NextChildGuidance(ctx, db, run.Turn)
				if err != nil {
					return err
				}
				changes = append(changes, guidanceEvents...)
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
			break
		}
		unlock()
		unlock = nil
		if !reported {
			reported = true
			logging.Emit(slog.LevelError, slog.String("area", "agent"), slog.String("phase", logging.Failed), slog.String("operation", "finish"),
				slog.String("session", x.inv.Session.ID.String()), slog.String("turn", x.inv.Run.Turn.String()), slog.String("exception", err.Error()))
		}
		// A stopping Node leaves the run to restart recovery.
		if r.clock.Sleep(r.ctx, settlementRetry) != nil {
			return
		}
	}
	r.observeTurn(run, x.inv.Turn.Config.Mode, child != nil)
	if successor != nil {
		next := &delegatedChild{parent: child.parent, session: child.session, turn: successor.Turn, sequence: successor.Sequence, ctx: child.ctx, cancel: child.cancel, released: true}
		r.mu.Lock()
		child.successor = next
		delete(r.children, child.turn)
		r.children[next.turn] = next
		r.mu.Unlock()
	}
	r.publish()
	if reviewEntry != nil {
		r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "entry", Data: *reviewEntry})
	}
	if reviewQuestion != nil {
		r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "question", Data: *reviewQuestion})
	}
	r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
	r.RefreshCommitted(run.Session)
	if x.inv.Session.Delegation != nil && *x.inv.Session.Delegation != nil {
		r.RefreshCommitted((*x.inv.Session.Delegation).Session)
	}
	// Packages observe every ended main task turn, whatever its status, once
	// its end is stored and published. A stopping Node calls none.
	unlock()
	unlock = nil
	if r.afterTurn != nil && r.ctx.Err() == nil && child == nil && x.inv.Session.Delegation == nil && x.inv.Run.Kind == protocol.ConversationRunKindTask {
		x.observedMu.Lock()
		observed := x.observed
		x.observedMu.Unlock()
		r.afterTurn(r.ctx, x.inv, run, observed)
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
	if err := r.captureResources(x); err != nil {
		return err
	}
	if r.authorizeProvider != nil {
		if err := r.authorizeProvider(x.ctx, inv); err != nil {
			return err
		}
	}
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
	instructions := map[string]bool{}
	for _, tool := range x.tools {
		if tool.Instruction != "" && !instructions[tool.Instruction] {
			policy += "\n\n" + tool.Instruction
			instructions[tool.Instruction] = true
		}
	}
	if x.assistantContext != "" {
		policy += "\n\n" + x.assistantContext
	}
	if x.rolePolicy != "" {
		policy += "\n\n" + x.rolePolicy
	}
	if x.modeContext != "" {
		policy += "\n\n" + x.modeContext
	}
	if inv.Session.Delegation == nil {
		if lead := strings.TrimSpace(inv.Turn.Roles.Instructions); lead != "" {
			policy += "\n\nTeam instructions\n" + lead
		}
	}
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
		record := inputRecord(inv.Run.Turn, inv.Message, r)
		if inv.Session.Delegation != nil {
			source := canonical.InputParent
			record.Metadata.Input.Source = &source
		}
		if err := r.bindBackground(x, &record); err != nil {
			return err
		}
		initial = append(initial, record)
	}
	if err = r.save(x.ctx, inv.Session.ID, initial...); err != nil {
		return err
	}
	recoveries := 0
	for {
		if err = x.ctx.Err(); err != nil {
			return err
		}
		state, err = r.reload(x.ctx, inv.Session.ID, state)
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
		// Every input waiting for this turn joins here as one batch: after tool
		// results or a finished reply, before the next model step. A failed step
		// takes none; the turn ends and its guidance returns to the queue.
		if failed := lastMessage(state.Records); failed == nil || failed.Error == nil {
			joined, err := r.guidance(x)
			if err != nil {
				return err
			}
			if joined {
				x.foregroundSteered = true
				continue
			}
		}
		selected, err := r.selectedAgent(x, state)
		if err != nil {
			return err
		}
		if selected {
			continue
		}
		delivered, err := r.foregroundStep(x)
		if err != nil {
			return err
		}
		if delivered {
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
			failure := &model.Failure{Kind: model.Other, Message: last.Error.Message}
			if last.Error.Code != nil {
				failure.Kind = model.FailureKind(*last.Error.Code)
			}
			return failure
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
		limited, err := r.childTurnLimit(x, state)
		if err != nil {
			return err
		}
		if limited {
			return nil
		}
		observed := map[string]bool{}
		for _, tool := range x.tools {
			if tool.BeforeModel == nil || tool.Binding == nil || observed[tool.Binding.Owner.Name] {
				continue
			}
			observed[tool.Binding.Owner.Name] = true
			stopped, err := tool.BeforeModel(x.ctx, state.Records)
			if err != nil {
				return err
			}
			if stopped {
				return nil
			}
		}
		if err = r.infer(x, binding, languageModel, state); err != nil {
			return err
		}
		x.foregroundSteered = false
	}
}

func (r *Runtime) load(ctx context.Context, session protocol.SessionID) (store.AgentState, error) {
	return r.reload(ctx, session, store.AgentState{})
}

// reload decodes and hydrates only the records appended since previous. The
// earlier records are shared with previous and are read-only for every step.
func (r *Runtime) reload(ctx context.Context, session protocol.SessionID, previous store.AgentState) (store.AgentState, error) {
	var state store.AgentState
	err := r.db.Read(ctx, func(db *sql.Conn) error {
		var reused int
		var err error
		state, reused, err = store.ExtendAgentState(ctx, db, session, previous)
		if err != nil {
			return err
		}
		for i := reused; i < len(state.Records); i++ {
			if message := state.Records[i].MutableMessage(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok {
						if err := hydrateToolResult(ctx, db, result); err != nil {
							return err
						}
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
			r.captureCorePresentation(&record)
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
