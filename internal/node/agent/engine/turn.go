package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"maps"
	"os"
	"runtime"
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

var errSettleStoppedChildren = errors.New("settle children before finishing a stopped turn")

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
	stoppedBeforeSettling := x.ctx.Err() != nil
	childrenSettled := r.cancelForeground(x)
	settledStop := stoppedBeforeSettling && childrenSettled
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
		if hookErr := x.hooks.Complete(x.ctx, hookStatus); hookErr != nil && (x.ctx.Err() == nil || !turnEndHookCancelled(hookErr)) {
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
	// The turn keeps its owner until its end is durable. Retries do not run
	// the model, tools, hooks or cleanup again.
	reported := false
	finishUnknown := false
	var unlock func()
	defer func() {
		if unlock != nil {
			unlock()
		}
	}()
	finish := func(db *sql.Conn) error {
		successor, reviewEntry, reviewQuestion = nil, nil, nil
		var changes []protocol.Event
		var err error
		finishStatus := status
		if !childrenSettled {
			finishStatus = protocol.ConversationStatusInterrupted
		} else if status == protocol.ConversationStatusCompleted && x.ctx.Err() != nil {
			finishStatus = r.stoppedStatus()
		}
		run, changes, err = store.FinishTurn(ctx, db, x.inv.Run.Turn, finishStatus, failure, r.clock.Now())
		if err != nil {
			return err
		}
		// A Stop committed during cleanup or the completion hook may not
		// have reached the runtime yet. FinishTurn applies the durable
		// decision. Roll back this write so children can finish first.
		if run.Status == protocol.ConversationStatusCancelled && !settledStop && x.inv.Session.Delegation == nil {
			return errSettleStoppedChildren
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
	}
	for {
		unlock = r.LockPublication()
		if finishUnknown {
			// An uncertain commit may already have ended this turn, including
			// its guidance and review changes. Inspect it before another write.
			var current protocol.ConversationRun
			err = r.db.Read(r.ctx, func(db *sql.Conn) error {
				var err error
				current, err = store.ReadAgentRun(r.ctx, db, x.inv.Run.Turn)
				return err
			})
			if err == nil {
				if current.FinishedMs != nil {
					run = current
					break
				}
				finishUnknown = false
			}
		}
		if !finishUnknown {
			err = r.db.Write(ctx, finish)
			if err == nil {
				// A recovered commit can already have a successor. Handoff
				// requires confirmation under the same publication lock.
				if run.Status == protocol.ConversationStatusCompleted {
					r.responsesHistory.Complete(x.ctx)
				}
				break
			}
			fault, ok := errors.AsType[*protocol.Fault](err)
			finishUnknown = ok && fault.Code == protocol.ErrorCodeOutcomeUnknown
		}
		unlock()
		unlock = nil
		if errors.Is(err, errSettleStoppedChildren) {
			x.cancel()
			r.stopChildren(x)
			if !r.settleChildren(x, nil) {
				return
			}
			settledStop = true
			continue
		}
		// A stopping Node leaves the run to restart recovery.
		if !r.waitSettlementRetry(x, "finish", err, &reported) {
			return
		}
	}
	if run.Status == protocol.ConversationStatusCancelled && x.inv.Session.Delegation == nil {
		// The running turn can observe a committed stop and settle before the
		// notifier reaches it. Its successor still waits on publication.
		r.stopTurnCommands(run.Session, x)
	}
	r.observeTurn(run, x.inv.Turn.Config.Mode, child != nil)
	if successor != nil {
		next := &delegatedChild{
			root: child.root, parent: child.parent, session: child.session, turn: successor.Turn, sequence: successor.Sequence,
			ctx: child.ctx, cancel: child.cancel, released: true, overlap: child.overlap, detached: child.detached,
			commandsInherited: true, tools: child.tools, connections: child.connections,
		}
		r.mu.Lock()
		child.successor = next
		delete(r.children, child.turn)
		r.children[next.turn] = next
		r.mu.Unlock()
	} else {
		r.stopChildCommands(x, run)
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
		parent := (*x.inv.Session.Delegation).Session
		if successor != nil {
			r.RefreshCommitted(parent, run.Turn, successor.Turn)
		} else {
			r.RefreshCommitted(parent, run.Turn)
		}
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

func turnEndHookCancelled(err error) bool {
	// The hook binding reports a recorded process cancellation as unavailable.
	// Match the exact fault, not a combined hook or receipt failure.
	if fault, ok := errors.AsType[*protocol.Fault](err); ok {
		return fault.Code == protocol.ErrorCodeUnavailable && fault.Message == string(protocol.HooksPhaseTurnEnd)+" hook failed: cancelled"
	}
	return errors.Is(err, context.Canceled)
}

func (r *Runtime) waitSettlementRetry(x *execution, operation string, err error, reported *bool) bool {
	if r.ctx.Err() != nil {
		return false
	}
	if !*reported {
		*reported = true
		logging.Emit(slog.LevelError, slog.String("area", "agent"), slog.String("phase", logging.Failed), slog.String("operation", operation),
			slog.String("session", x.inv.Session.ID.String()), slog.String("turn", x.inv.Run.Turn.String()), slog.String("exception", err.Error()))
	}
	return r.clock.Sleep(r.ctx, settlementRetry) == nil
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
	defer func() { x.streamReads.close() }()
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
	if capturedModel(binding).Tools {
		if addition := capabilityInstructions(x, policy); addition != "" {
			policy += "\n\n" + addition
		}
	}
	// Full permission and unsupported systems run commands outside the sandbox
	// whatever the setting says; the model is told the effective state.
	sandbox := protocol.DefaultSandboxSettings()
	if err = r.db.Read(x.ctx, func(db *sql.Conn) error {
		var err error
		sandbox, err = store.ReadSandboxSettings(x.ctx, db)
		return err
	}); err != nil {
		return err
	}
	sandboxed := sandbox.Enabled && inv.Turn.Config.Permission != protocol.PermissionFull && (runtime.GOOS == "darwin" || runtime.GOOS == "linux")
	commands := capturedModel(binding).Tools && findTool(x.tools, "run_command") != nil
	environment := prompt.Environment(r.clock.Now(), inv.Turn.Config.Model, os.Getenv("SHELL"), inv.Worktree.Project != nil, commands, sandboxed)
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
		if err := validateInstructionRecord(record); err != nil {
			return err
		}
		initial = append(initial, record)
	}
	if inv.Run.Kind != protocol.ConversationRunKindCompaction {
		current := canonical.Context{Policy: &policy, Personal: &personal, Guides: maps.Clone(state.Context.Guides)}
		if current.Guides == nil {
			current.Guides = map[string]canonical.Guide{}
		}
		for key, guide := range guides {
			current.Guides[key] = guide
		}
		if err := validateInstructionContext(binding, current, inv.Session.Delegation != nil); err != nil {
			return err
		}
		if !inv.Run.Notice {
			record := inputRecord(inv.Run.Turn, inv.Message, r)
			if inv.Session.Delegation != nil {
				source := canonical.InputParent
				record.Metadata.Input.Source = &source
			}
			initial = append(initial, record)
		}
	}
	if err = r.save(x.ctx, inv.Session.ID, initial...); err != nil {
		return err
	}
	if inv.Run.Kind == protocol.ConversationRunKindTask {
		delivered, err := r.deliverBackground(x, true)
		if err != nil {
			return err
		}
		if inv.Run.Notice && !delivered {
			return nil
		}
	}
	recoveries := 0
	compacted := false
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
			// A reviewed plan ends the turn without another model call; its
			// running children deliver their results later.
			return r.detachForeground(x)
		}
		// Each applicable operation commits before the next state reload.
		if calls := pending(state.Records); len(calls) > 0 {
			if err = r.runTools(x, state, calls); err != nil {
				return err
			}
			continue
		}
		// One batch joins after tool results or a finished reply, before the
		// next inference. A failed step leaves its guidance in the queue.
		if failed := lastMessage(state.Records); !x.foregroundSteered && (failed == nil || failed.Error == nil) {
			joined, err := r.guidance(x)
			if err != nil {
				return err
			}
			if joined {
				x.foregroundSteered = true
				x.doomSignature = ""
				x.doomStreak = 0
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
		last := lastMessage(state.Records)
		finishing := last != nil && last.Role == canonical.Assistant && last.Error == nil
		// A completed child can reopen a reply while its siblings are running.
		// A failed step takes no result: no model call would answer it.
		if last == nil || last.Error == nil {
			delivered, err := r.foregroundStep(x, finishing && !x.callbackPending)
			if err != nil {
				return err
			}
			if delivered {
				continue
			}
		}
		if last != nil && last.Error != nil {
			if last.Error.Code != nil && *last.Error.Code == "context_overflow" && recoveries < 2 {
				if err = r.compact(x, binding, state, false); err != nil {
					return err
				}
				recoveries++
				compacted = true
				continue
			}
			// The model failed, not the delegated work: children keep running
			// and report to the next turn instead of being cancelled with it.
			if err = r.detachForeground(x); err != nil {
				return err
			}
			failure := &model.Failure{Kind: model.Other, Message: last.Error.Message, Exhausted: last.Error.RetryExhausted, WaitExhausted: last.Error.RetryWaitExhausted}
			if last.Error.Code != nil {
				failure.Kind = model.FailureKind(*last.Error.Code)
			}
			return failure
		}
		if !x.callbackPending && inv.Run.Kind == protocol.ConversationRunKindTask {
			delivered, err := r.deliverBackground(x, false)
			if err != nil {
				return err
			}
			if delivered {
				state, err = r.reload(x.ctx, inv.Session.ID, state)
				if err != nil {
					return err
				}
			}
		}
		if !compacted && shouldCompact(binding, state, inv.Compaction) {
			if err = r.compact(x, binding, state, false); err != nil {
				return err
			}
			// The carried user input can itself exceed the threshold. Try the
			// main request before considering another threshold compaction.
			compacted = true
			continue
		}
		if finishing && !x.callbackPending {
			if r.hasQueuedSteer(x) || r.hasDirectSteer(x) {
				continue
			}
			return nil
		}
		limited, err := r.childTurnLimit(x, state)
		if err != nil {
			return err
		}
		if limited {
			return nil
		}
		if inv.Session.Delegation == nil && r.maxModelSteps > 0 && x.modelSteps >= r.maxModelSteps {
			if err = r.detachForeground(x); err != nil {
				return err
			}
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("turn stopped after reaching the %d-step safety limit", r.maxModelSteps)}
		}
		observed := map[string]bool{}
		readResult := func(response *canonical.ToolResponse) (json.RawMessage, error) {
			return r.readToolResult(x.ctx, response)
		}
		for _, tool := range x.tools {
			if tool.BeforeModel == nil {
				continue
			}
			var ownerKey string
			switch {
			case tool.Binding != nil && tool.Binding.Owner.Name != "":
				ownerKey = "plugin:" + tool.Binding.Owner.Name
			case tool.LegacyPackage != "":
				ownerKey = "native:" + tool.LegacyPackage
			default:
				ownerKey = "tool:" + tool.Definition.Name
			}
			if observed[ownerKey] {
				continue
			}
			observed[ownerKey] = true
			stopped, err := tool.BeforeModel(x.ctx, state.Records, readResult)
			if err != nil {
				return err
			}
			if stopped {
				return r.settleForeground(x)
			}
		}
		x.modelSteps++
		if err = r.infer(x, binding, languageModel, state); err != nil {
			if err == errQueuedSteer {
				x.modelSteps--
				continue
			}
			return err
		}
		compacted = false
		x.foregroundSteered = false
		x.callbackPending = false
	}
}

func (r *Runtime) load(ctx context.Context, session protocol.SessionID) (store.AgentState, error) {
	return r.reload(ctx, session, store.AgentState{})
}

// reload decodes appended records without materializing their binary payloads.
// Earlier records are shared with previous and stay read-only for every step.
func (r *Runtime) reload(ctx context.Context, session protocol.SessionID, previous store.AgentState) (store.AgentState, error) {
	var state store.AgentState
	err := r.db.Read(ctx, func(db *sql.Conn) error {
		var err error
		state, _, err = store.ExtendAgentState(ctx, db, session, previous)
		return err
	})
	return state, err
}

func (r *Runtime) save(ctx context.Context, session protocol.SessionID, records ...canonical.Record) error {
	return r.saveWithProvider(ctx, session, nil, records...)
}

func (r *Runtime) saveWithProvider(ctx context.Context, session protocol.SessionID, provider *protocol.ConversationProvider, records ...canonical.Record) error {
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
			entry, events, err := store.AppendCanonicalWithProvider(ctx, db, session, record, provider)
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
	turns := make([]protocol.TurnID, len(entries))
	for i, entry := range entries {
		turns[i] = entry.Turn
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
	r.refreshStatisticsForTurnsCommitted(session, turns)
	return nil
}

func (r *Runtime) guidance(x *execution) (bool, error) {
	ctx := x.ctx
	unlock := r.LockPublication()
	defer unlock()
	var entries []protocol.ConversationEntry
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		values, changes, err := store.TakeGuidance(ctx, db, x.inv.Run.Turn, x.queuedSteer...)
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
	if err == nil {
		x.queuedSteer = nil
		x.directSteer = false
	}
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
		if background := records[i].Background(); background != nil && background.Initial {
			return nil
		}
		if message := records[i].Message(); message != nil {
			if records[i].Metadata.Runtime == nil || records[i].Metadata.Runtime.AgentVisible || message.Error != nil {
				return message
			}
		}
	}
	return nil
}

func pending(records []canonical.Record) []canonical.ToolRequest {
	return pendingCalls(records, nil)
}

// When settling an interrupted turn, collect durable evidence that execution
// was possible. A reused call ID begins a new observation after its response.
func pendingCalls(records []canonical.Record, admitted map[string]bool) []canonical.ToolRequest {
	start := 0
	for i := range records {
		if background := records[i].Background(); background != nil && background.Initial {
			start = i
			continue
		}
		input := records[i].Metadata.Input
		if m := records[i].Message(); m != nil && m.Role == canonical.User && (records[i].Provenance.Author == "user" || input != nil && input.Source != nil && *input.Source == canonical.InputNotice) {
			start = i
		}
	}
	var requests []*canonical.ToolRequest
	waiting := map[string][]int{}
	for i := start; i < len(records); i++ {
		if admitted != nil && records[i].Kind == "operation" {
			op, ok := records[i].Data.(*canonical.Operation)
			if !ok {
				if v, ok := records[i].Data.(canonical.Operation); ok {
					op = &v
				}
			}
			if op != nil && op.Kind == "tool_meta" {
				if meta, ok := op.Data.(*canonical.ToolMeta); ok && len(waiting[meta.Call]) > 0 {
					var mark map[string]json.RawMessage
					if json.Unmarshal(meta.Patch, &mark) == nil && string(mark[canonical.ExecutableKey]) == "true" {
						admitted[meta.Call] = true
					}
				}
			}
		}
		if m := records[i].Message(); m != nil {
			for _, block := range m.Content {
				switch value := block.Data.(type) {
				case *canonical.ToolRequest:
					if len(waiting[value.ID]) == 0 {
						delete(admitted, value.ID)
					}
					waiting[value.ID] = append(waiting[value.ID], len(requests))
					requests = append(requests, value)
				case *canonical.ToolResponse:
					if matches := waiting[value.ID]; len(matches) > 0 {
						requests[matches[0]] = nil
						waiting[value.ID] = matches[1:]
					}
				}
			}
		}
	}
	var calls []canonical.ToolRequest
	for _, request := range requests {
		if request == nil {
			continue
		}
		if !externalToolCall(*request) {
			calls = append(calls, *request)
		}
	}
	return calls
}

func externalToolCall(call canonical.ToolRequest) bool {
	if len(call.ToolMeta) == 0 {
		return false
	}
	var meta map[string]json.RawMessage
	_ = json.Unmarshal(call.ToolMeta, &meta)
	return string(meta[canonical.ExternalDispatchKey]) == "true"
}
