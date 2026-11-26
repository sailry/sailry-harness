package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"
	"os"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type delegatedChild struct {
	// root is the main session that owns the child. A successor inherits its
	// place through the chain of the child it follows.
	root       protocol.SessionID
	parent     protocol.TurnID
	session    protocol.SessionID
	turn       protocol.TurnID
	sequence   uint64
	ctx        context.Context
	cancel     context.CancelFunc
	stopParent func() bool
	started    bool
	released   bool
	overlap    bool
	// detached children left a turn that made no further model call. Their
	// results follow up like a background child's.
	detached  bool
	successor *delegatedChild
	// A queued successor owns its predecessor's commands even before it starts.
	commandsInherited bool
	tools             inheritedTools
	connections       []protocol.ConnectionResource
}

type collectedChild struct {
	session protocol.SessionID
	turn    protocol.TurnID
}

type childMode uint8

const (
	childForeground childMode = iota
	childOverlap
	childBackground
)

func (r *Runtime) activitySignal() <-chan struct{} {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.activity == nil {
		r.activity = make(chan struct{})
	}
	return r.activity
}

// admitChild keeps the Node's existing operation lease until durable admission
// and handoff finish, even if the calling tool stops waiting for the receipt.
// arguments are the ones that execute and name the child's workspace.
func (r *Runtime) admitChild(x *execution, arguments json.RawMessage, mode childMode, admit func(context.Context, *sql.Conn) (*store.AgentInvocation, []protocol.Event, error)) ToolResult {
	if x.delegationTools == nil {
		return toolFault(&protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "parent tool capture is unavailable"})
	}
	parent, release, ok := r.BeginOperation(x.inv.Run.Turn)
	if !ok {
		return invalidTool("parent execution has ended")
	}
	returned := make(chan ToolResult, 1)
	r.workers.Add(1)
	go func() {
		defer r.workers.Done()
		defer release()
		ctx := store.WithCapturedUser(store.WithUser(r.ctx, x.inv.User), x.inv.StorageUser)
		if err := r.preflightChildWorkspace(parent, x, arguments); err != nil {
			returned <- toolFault(err)
			return
		}
		unlock := r.LockPublication()
		defer unlock()
		var inv *store.AgentInvocation
		err := r.db.Write(ctx, func(db *sql.Conn) error {
			var events []protocol.Event
			var err error
			inv, events, err = admit(ctx, db)
			if err != nil {
				return err
			}
			if inv != nil {
				encodedTools, encErr := encodeInheritedTools(x.delegationTools)
				if encErr != nil {
					return encErr
				}
				if err = store.SaveDelegatedTools(ctx, db, inv.Session.ID, encodedTools); err != nil {
					return err
				}
			}
			_, err = store.RecordEvents(ctx, db, r.node, events)
			return err
		})
		if err != nil {
			returned <- toolFault(err)
			return
		}
		if inv == nil {
			r.publish()
			r.RefreshCommitted(x.inv.Session.ID)
			returned <- ToolResult{}
			r.Wake()
			return
		}
		background := mode == childBackground
		childContext, cancel := context.WithCancel(r.ctx)
		child := &delegatedChild{
			root: x.inv.Session.ID, parent: x.inv.Run.Turn, session: inv.Session.ID, turn: inv.Run.Turn, sequence: inv.Run.Sequence,
			ctx: childContext, cancel: cancel, released: background, overlap: mode == childOverlap,
			tools: x.delegationTools, connections: x.inv.ConnectionTargets,
		}
		if !background {
			// A foreground child ends with its turn unless the turn detached it.
			child.stopParent = context.AfterFunc(parent, func() {
				r.mu.Lock()
				detached := child.detached
				r.mu.Unlock()
				if !detached {
					cancel()
				}
			})
		}
		r.mu.Lock()
		if r.children == nil {
			r.children = map[protocol.TurnID]*delegatedChild{}
		}
		r.children[child.turn] = child
		if !background {
			x.foreground = append(x.foreground, child)
		}
		r.mu.Unlock()
		context.AfterFunc(childContext, r.Wake)
		if parent.Err() != nil {
			cancel()
		}
		r.publish()
		r.RefreshCommitted(x.inv.Session.ID)
		r.RefreshCommitted(inv.Session.ID)
		result := ToolResult{Value: map[string]any{"admitted": true, "pending": true, "session": child.session, "turn": child.turn, "status": "queued"}}
		if !background {
			session, _ := json.Marshal(child.session.String())
			value := map[string]json.RawMessage{"foreground_subagent": json.RawMessage("true"), "subagent_session_id": session}
			result.Meta = &value
		}
		returned <- result
		r.Wake()
	}()
	select {
	case value := <-returned:
		return value
	case <-x.ctx.Done():
		return toolFault(&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "child admission outcome is unknown"})
	}
}

// claimInvocation extends the existing claim pass. Every admitted child enters
// the same execute/turn pipeline after Store grants its FIFO concurrency slot.
// Ended owners are released by the caller only after the writer commits.
func (r *Runtime) claimInvocation(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, []*delegatedChild, error) {
	inv, events, err := store.ClaimNext(ctx, db, r.clock.Now(), slices.Collect(maps.Keys(r.afterTurnSessions))...)
	if err != nil || inv != nil {
		return inv, events, nil, err
	}
	// A queued child stopped by a controller is finished here; its owner joins
	// the ended owners returned below.
	settled, err := store.SettleStoppedChildren(ctx, db, r.clock.Now())
	if err != nil {
		return nil, nil, nil, err
	}
	events = append(events, settled...)
	r.mu.Lock()
	children := make([]*delegatedChild, 0, len(r.children))
	for _, child := range r.children {
		if !child.started {
			children = append(children, child)
		}
	}
	r.mu.Unlock()
	slices.SortFunc(children, func(a, b *delegatedChild) int {
		if a.sequence < b.sequence {
			return -1
		}
		if a.sequence > b.sequence {
			return 1
		}
		return 0
	})
	var ended []*delegatedChild
	for _, child := range children {
		run, err := store.ReadAgentRun(ctx, db, child.turn)
		if err != nil {
			return nil, nil, nil, err
		}
		if !childActive(run.Status) {
			ended = append(ended, child)
			continue
		}
		if child.ctx.Err() != nil {
			status := protocol.ConversationStatusCancelled
			if r.ctx.Err() != nil {
				status = protocol.ConversationStatusInterrupted
			}
			changed, err := store.FinishQueuedChild(ctx, db, child.turn, status, nil, r.clock.Now())
			if err != nil {
				return nil, nil, nil, err
			}
			events = append(events, changed...)
			ended = append(ended, child)
			continue
		}
		claimed, changed, err := store.ClaimChild(ctx, db, child.turn, r.clock.Now())
		if err != nil {
			return nil, nil, nil, err
		}
		events = append(events, changed...)
		if claimed != nil {
			return claimed, events, ended, nil
		}
	}
	retried, changed, err := store.ClaimRetriedChild(ctx, db, r.clock.Now())
	if err != nil {
		return nil, nil, nil, err
	}
	events = append(events, changed...)
	return retried, events, ended, nil
}

// startClaimedChild runs after the claim commits, under publication ownership.
// A controller retry does not belong to an old turn's lifetime, but a Stop in
// its parent session must still cancel it and wait for its settlement.
func (r *Runtime) startClaimedChild(inv *store.AgentInvocation) (context.Context, inheritedTools) {
	r.mu.Lock()
	child := r.children[inv.Run.Turn]
	if child != nil {
		child.started = true
	}
	r.mu.Unlock()
	if child != nil {
		inv.ConnectionTargets = child.connections
		return child.ctx, child.tools
	}
	if inv.Session.Delegation == nil || *inv.Session.Delegation == nil {
		return r.ctx, nil
	}
	origin := *inv.Session.Delegation
	var tools inheritedTools
	var rawTools []byte
	if err := r.db.Read(r.ctx, func(db *sql.Conn) error {
		var err error
		rawTools, err = store.ReadDelegatedTools(r.ctx, db, inv.Session.ID)
		return err
	}); err == nil && len(rawTools) > 0 {
		if decoded, err := decodeInheritedTools(rawTools); err == nil {
			tools = decoded
		}
	}
	ctx, cancel := context.WithCancel(r.ctx)
	child = &delegatedChild{
		root: origin.Session, parent: origin.Turn, session: inv.Session.ID, turn: inv.Run.Turn, sequence: inv.Run.Sequence,
		ctx: ctx, cancel: cancel, started: true, released: true, detached: true,
		tools: tools, connections: inv.ConnectionTargets,
	}
	r.mu.Lock()
	r.children[child.turn] = child
	r.mu.Unlock()
	return child.ctx, child.tools
}

// preflightChildWorkspace checks the worktree the child will run in before it
// is admitted. arguments are the prepared ones: a package tool bound to
// agents.delegate or agents.guide chooses the child there, not in its own call.
func (r *Runtime) preflightChildWorkspace(ctx context.Context, x *execution, arguments json.RawMessage) error {
	var args struct {
		Worktree *protocol.WorktreeID `json:"worktree"`
		Session  *protocol.SessionID  `json:"session"`
	}
	if json.Unmarshal(arguments, &args) != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid child workspace arguments"}
	}
	id := x.inv.Run.Worktree
	if args.Worktree != nil {
		id = *args.Worktree
	} else if args.Session != nil {
		if err := r.db.Read(ctx, func(db *sql.Conn) error {
			state, err := store.ReadChildState(ctx, db, x.inv.Run.Turn, *args.Session, nil)
			id = state.Run.Worktree
			return err
		}); err != nil {
			return err
		}
	}
	if r.validateWorktree != nil {
		return r.validateWorktree(ctx, id)
	}
	var directory string
	if err := r.db.Read(ctx, func(db *sql.Conn) error {
		var err error
		directory, err = store.ReadWorkspaceRoot(ctx, db, id, false)
		return err
	}); err != nil {
		return err
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "child worktree directory is unavailable"}
	}
	return root.Close()
}

func (r *Runtime) settleQueuedChildren() {
	r.mu.Lock()
	var children []*delegatedChild
	for _, child := range r.children {
		child.cancel()
		if !child.started {
			children = append(children, child)
		}
	}
	r.mu.Unlock()
	if len(children) == 0 {
		return
	}
	ctx := context.WithoutCancel(r.ctx)
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		for _, child := range children {
			changed, err := store.FinishQueuedChild(ctx, db, child.turn, protocol.ConversationStatusInterrupted, nil, r.clock.Now())
			if err != nil {
				return err
			}
			events = append(events, changed...)
		}
		_, err := store.RecordEvents(ctx, db, r.node, events)
		return err
	})
	if err == nil {
		r.publish()
	}
}

func (r *Runtime) foregroundStep(x *execution, finishing bool) (bool, error) {
	delivered, guidance, err := r.waitForeground(x, finishing, true)
	return delivered || guidance, err
}

// waitForeground delivers the next settled foreground child. With
// acceptGuidance, pending guidance for the turn ends the wait instead.
func (r *Runtime) waitForeground(x *execution, finishing, acceptGuidance bool) (delivered, guidance bool, err error) {
	r.mu.Lock()
	children := slices.Clone(x.foreground)
	r.mu.Unlock()
	if len(children) == 0 {
		return false, false, nil
	}
	var release []protocol.TurnID
	for _, child := range children {
		if !child.released {
			release = append(release, child.turn)
		}
	}
	if len(release) > 0 {
		unlock := r.LockPublication()
		err := r.db.Write(x.ctx, func(db *sql.Conn) error {
			events, err := store.ReleaseChildren(x.ctx, db, x.inv.Run.Turn, release)
			if err != nil {
				return err
			}
			_, err = store.RecordEvents(x.ctx, db, r.node, events)
			return err
		})
		if err == nil {
			for _, child := range children {
				child.released = true
			}
			r.publish()
		}
		unlock()
		if err != nil {
			return false, false, err
		}
		r.Wake()
	}
	var stopWaiting func()
	defer func() {
		if stopWaiting != nil {
			stopWaiting()
		}
	}()
	for {
		changed := r.activitySignal()
		if acceptGuidance && !x.foregroundSteered {
			var guidance bool
			err := r.db.Read(x.ctx, func(db *sql.Conn) error {
				var err error
				guidance, err = store.PendingGuidance(x.ctx, db, x.inv.Run.Turn)
				return err
			})
			if err != nil {
				return false, false, err
			}
			if guidance {
				// The loop continues so the guidance joins before the next request.
				return false, true, nil
			}
		}
		pending := false
		deliveredAny := false
		for _, child := range children {
			var state store.ChildState
			unlock := r.LockPublication()
			r.mu.Lock()
			current := child
			for current.successor != nil {
				current = current.successor
			}
			r.mu.Unlock()
			err := r.db.Read(x.ctx, func(db *sql.Conn) error {
				var err error
				state, err = store.ReadChildState(x.ctx, db, x.inv.Run.Turn, child.session, &current.turn)
				return err
			})
			unlock()
			if err != nil {
				return deliveredAny, false, err
			}
			if childPending(state.Run) {
				pending = pending || finishing || !current.overlap
				continue
			}
			if err := r.deliverForeground(x, state); err != nil {
				return deliveredAny, false, err
			}
			deliveredAny = true
		}
		if deliveredAny {
			return true, false, nil
		}
		if !pending || acceptGuidance && x.foregroundSteered {
			return false, false, nil
		}
		if stopWaiting == nil {
			stopWaiting = x.timeline.span(phaseExcluded, "foreground")
		}
		select {
		case <-x.ctx.Done():
			return false, false, x.ctx.Err()
		case <-changed:
		}
	}
}

// settleForeground keeps admitted results when a plugin ends inference: the
// turn collects its foreground children without another model call. Guidance
// cannot join such a turn, so it ends the wait. The remaining children then
// detach to background delivery and the guidance runs as the next turn.
func (r *Runtime) settleForeground(x *execution) error {
	x.foregroundSteered = false
	for {
		delivered, guidance, err := r.waitForeground(x, true, true)
		if err != nil {
			return err
		}
		if guidance {
			return r.detachForeground(x)
		}
		if !delivered {
			return nil
		}
	}
}

// detachForeground hands the turn's undelivered foreground children to
// background delivery when the turn makes no further model call. They keep
// running; each result arrives as a notice or with the next input instead
// of being cancelled with the turn.
func (r *Runtime) detachForeground(x *execution) error {
	r.mu.Lock()
	children := slices.Clone(x.foreground)
	r.mu.Unlock()
	if len(children) == 0 {
		return nil
	}
	turns := make([]protocol.TurnID, len(children))
	for i, child := range children {
		turns[i] = child.turn
	}
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		events, err := store.DetachChildren(x.ctx, db, x.inv.Run.Turn, turns)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(x.ctx, db, r.node, events)
		return err
	})
	if err != nil {
		return err
	}
	r.mu.Lock()
	x.foreground = slices.DeleteFunc(x.foreground, func(child *delegatedChild) bool { return slices.Contains(children, child) })
	for _, child := range children {
		child.detached, child.released = true, true
		child.unlinkParent()
		for current := child.successor; current != nil; current = current.successor {
			current.detached = true
		}
	}
	r.mu.Unlock()
	r.publish()
	r.RefreshCommitted(x.inv.Session.ID)
	r.Wake()
	return nil
}

func (r *Runtime) consumeChild(x *execution, session protocol.SessionID, turn *protocol.TurnID) error {
	ctx := context.WithoutCancel(x.ctx)
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		events, err := store.ConsumeChildBackground(ctx, db, x.inv.Run.Turn, session, turn)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, r.node, events)
		return err
	})
	if err == nil {
		// Explicit wait_agent results and automatic delivery share the same
		// completion state. Collecting an older turn must not hide a successor.
		r.markCollectedChild(x, session, turn)
		r.publish()
		r.RefreshCommitted(x.inv.Session.ID)
	}
	return err
}

func (r *Runtime) markCollectedChild(x *execution, session protocol.SessionID, turn *protocol.TurnID) {
	r.mu.Lock()
	defer r.mu.Unlock()
	x.foreground = slices.DeleteFunc(x.foreground, func(child *delegatedChild) bool {
		if child.session != session || turn == nil || child.last().turn != *turn {
			return false
		}
		child.unlinkParent()
		return true
	})
}

func (c *delegatedChild) unlinkParent() {
	if c.stopParent != nil {
		c.stopParent()
		c.stopParent = nil
	}
}

// stopChildren cancels every child of the turn's session, foreground or
// background, including those earlier turns left running. Children cannot
// delegate, so the session's children are its whole subtree. A failed turn
// keeps the children it detached; a stop cancels them too. The turn keeps
// the children it cancelled: one that finishes and leaves the runtime before
// the turn settles them still has its completion record consumed.
func (r *Runtime) stopChildren(x *execution) {
	stopped := x.ctx.Err() != nil
	r.mu.Lock()
	for _, child := range r.children {
		if child.root == x.inv.Session.ID && (stopped || !child.detached) {
			child.cancel()
			if !slices.Contains(x.stopped, child) {
				x.stopped = append(x.stopped, child)
			}
		}
	}
	r.mu.Unlock()
}

// cancelForeground ends the delegated work a finishing turn leaves behind.
// Undelivered foreground children are cancelled and noted. A stopped or failed
// turn has also cancelled every other child of its session. The turn waits
// until all of them have settled, so it never finishes ahead of its children.
func (r *Runtime) cancelForeground(x *execution) bool {
	if x.ctx.Err() != nil {
		r.stopChildren(x)
	}
	r.mu.Lock()
	children := slices.Clone(x.foreground)
	r.mu.Unlock()
	var ids []string
	for _, child := range children {
		child.cancel()
		ids = append(ids, child.session.String())
	}
	if len(ids) > 0 {
		value := &canonical.Note{Note: canonical.OperationNote{Kind: "subagents_cancelled", Data: &canonical.SubagentsCancelled{Sessions: ids}}}
		record := canonical.NewRecord(x.inv.Run.Turn, "operation", &canonical.Operation{Kind: "note", Data: value}, r.clock.Now())
		record.Provenance.Author = "system"
		record.Metadata.Runtime = &canonical.Runtime{Notes: &canonical.OperationNotes{CancelledSubagents: ids}}
		_ = r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, record)
	}
	settled := r.settleChildren(x, children)
	r.Wake()
	return settled
}

// settleChildren waits for the session's cancelled children to finish, then
// consumes their completion records: the stopped work is never reported again
// with a later message or by a notice turn. Only the storage work is retried.
// It returns false on Node shutdown, which settles children separately.
func (r *Runtime) settleChildren(x *execution, stopped []*delegatedChild) bool {
	ctx := r.ctx
	reported := false
	for ctx.Err() == nil {
		changed := r.activitySignal()
		r.mu.Lock()
		for _, child := range x.stopped {
			if !slices.Contains(stopped, child) {
				stopped = append(stopped, child)
			}
		}
		for _, child := range r.children {
			if child.root == x.inv.Session.ID && child.ctx.Err() != nil && !slices.Contains(stopped, child) {
				stopped = append(stopped, child)
			}
		}
		var queued []*delegatedChild
		var turns []protocol.TurnID
		running := false
		for _, child := range stopped {
			last := child.last()
			switch {
			case r.children[last.turn] != last:
				if !slices.Contains(turns, last.turn) {
					turns = append(turns, last.turn)
				}
			case last.started:
				running = true
			case !slices.Contains(queued, last):
				queued = append(queued, last)
			}
		}
		r.mu.Unlock()
		var err error
		switch {
		case len(queued) > 0:
			err = r.finishQueued(ctx, queued)
		case running:
			select {
			case <-changed:
			case <-ctx.Done():
			}
		case len(turns) == 0:
			return true
		default:
			err = r.consumeStoppedChildren(ctx, x, turns)
			if err == nil {
				return true
			}
		}
		if err != nil && !r.waitSettlementRetry(x, "settle_children", err, &reported) {
			break
		}
	}
	return false
}

func (r *Runtime) consumeStoppedChildren(ctx context.Context, x *execution, turns []protocol.TurnID) error {
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		// Only still-pending records are changed if an earlier commit was uncertain.
		events, err := store.ConsumeStoppedChildren(ctx, db, x.inv.Run.Turn, turns)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, r.node, events)
		return err
	})
	if err == nil {
		r.publish()
		r.RefreshCommitted(x.inv.Session.ID)
	}
	return err
}

// last is the end of the child's successor chain. The caller holds r.mu.
func (c *delegatedChild) last() *delegatedChild {
	for c.successor != nil {
		c = c.successor
	}
	return c
}

// finishQueued finishes cancelled children that never started. Publication
// ownership excludes the claim pass, so a child found unstarted here stays so.
func (r *Runtime) finishQueued(ctx context.Context, children []*delegatedChild) error {
	unlock := r.LockPublication()
	defer unlock()
	r.mu.Lock()
	children = slices.DeleteFunc(children, func(child *delegatedChild) bool {
		return r.children[child.turn] != child || child.started
	})
	r.mu.Unlock()
	if len(children) == 0 {
		return nil
	}
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		for _, child := range children {
			// FinishQueuedChild checks the stored finish time before writing, so
			// an uncertain commit is not applied twice.
			changed, err := store.FinishQueuedChild(ctx, db, child.turn, protocol.ConversationStatusCancelled, nil, r.clock.Now())
			if err != nil {
				return err
			}
			events = append(events, changed...)
		}
		_, err := store.RecordEvents(ctx, db, r.node, events)
		return err
	})
	if err != nil {
		return err
	}
	for _, child := range children {
		r.releaseQueuedChild(child)
	}
	r.publish()
	turns := make([]protocol.TurnID, 0, len(children))
	for _, child := range children {
		r.RefreshCommitted(child.session)
		turns = append(turns, child.turn)
	}
	r.RefreshCommitted(children[0].root, turns...)
	return nil
}

// releaseQueuedChild drops a settled owner under publication ownership, before
// another turn can reuse its session's background command scope.
func (r *Runtime) releaseQueuedChild(child *delegatedChild) {
	r.mu.Lock()
	if r.children[child.turn] != child {
		r.mu.Unlock()
		return
	}
	child.cancel()
	delete(r.children, child.turn)
	r.mu.Unlock()
	if child.commandsInherited && r.stopCommands != nil && r.ctx.Err() == nil {
		r.stopCommands(child.session)
	}
}
