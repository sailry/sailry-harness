package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type delegatedChild struct {
	// root is the main session that owns the child. A successor inherits its
	// place through the chain of the child it follows.
	root      protocol.SessionID
	parent    protocol.TurnID
	session   protocol.SessionID
	turn      protocol.TurnID
	sequence  uint64
	ctx       context.Context
	cancel    context.CancelFunc
	started   bool
	released  bool
	delivered bool
	successor *delegatedChild
}

type collectedChild struct {
	session protocol.SessionID
	turn    protocol.TurnID
}

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
func (r *Runtime) admitChild(x *execution, arguments json.RawMessage, background bool, admit func(context.Context, *sql.Conn) (*store.AgentInvocation, []protocol.Event, error)) ToolResult {
	parent, release, ok := r.BeginOperation(x.inv.Run.Turn)
	if !ok {
		return invalidTool("parent execution has ended")
	}
	returned := make(chan ToolResult, 1)
	r.workers.Add(1)
	go func() {
		defer r.workers.Done()
		defer release()
		ctx := r.ctx
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
		owner := parent
		if background {
			owner = r.ctx
		}
		childContext, cancel := context.WithCancel(owner)
		child := &delegatedChild{root: x.inv.Session.ID, parent: x.inv.Run.Turn, session: inv.Session.ID, turn: inv.Run.Turn, sequence: inv.Run.Sequence, ctx: childContext, cancel: cancel, released: background}
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
func (r *Runtime) claimInvocation(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
	inv, events, err := store.ClaimNext(ctx, db, r.clock.Now())
	if err != nil || inv != nil {
		return inv, events, err
	}
	// A queued child stopped by a controller is finished here; its owner is
	// dropped below with every other child that is no longer active.
	settled, err := store.SettleStoppedChildren(ctx, db, r.clock.Now())
	if err != nil {
		return nil, nil, err
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
	for _, child := range children {
		run, err := store.ReadAgentRun(ctx, db, child.turn)
		if err != nil {
			return nil, nil, err
		}
		if !childActive(run.Status) {
			child.cancel()
			r.mu.Lock()
			delete(r.children, child.turn)
			r.mu.Unlock()
			continue
		}
		if child.ctx.Err() != nil {
			status := protocol.ConversationStatusCancelled
			if r.ctx.Err() != nil {
				status = protocol.ConversationStatusInterrupted
			}
			changed, err := store.FinishQueuedChild(ctx, db, child.turn, status, nil, r.clock.Now())
			if err != nil {
				return nil, nil, err
			}
			events = append(events, changed...)
			r.mu.Lock()
			delete(r.children, child.turn)
			r.mu.Unlock()
			continue
		}
		claimed, changed, err := store.ClaimChild(ctx, db, child.turn, r.clock.Now())
		if err != nil {
			return nil, nil, err
		}
		events = append(events, changed...)
		if claimed != nil {
			return claimed, events, nil
		}
	}
	return nil, events, nil
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

func (r *Runtime) foregroundStep(x *execution) (bool, error) {
	if x.foregroundSteered {
		return false, nil
	}
	r.mu.Lock()
	children := slices.Clone(x.foreground)
	r.mu.Unlock()
	if len(children) == 0 {
		return false, nil
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
			return false, err
		}
		r.Wake()
	}
	for {
		changed := r.activitySignal()
		var guidance bool
		err := r.db.Read(x.ctx, func(db *sql.Conn) error {
			var err error
			guidance, err = store.PendingGuidance(x.ctx, db, x.inv.Run.Turn)
			return err
		})
		if err != nil {
			return false, err
		}
		if guidance {
			// The loop continues so the guidance joins before the next request.
			return true, nil
		}
		pending := false
		for _, child := range children {
			if child.delivered {
				continue
			}
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
				return false, err
			}
			if childPending(state.Run) {
				pending = true
				continue
			}
			text := "Subagent " + child.session.String() + " completed: "
			notes := &canonical.OperationNotes{}
			id := child.session.String()
			switch state.Run.Status {
			case protocol.ConversationStatusCompleted:
				if state.Response != nil {
					text += *state.Response
				}
				notes.DeliveredSubagent = &id
			case protocol.ConversationStatusCancelled, protocol.ConversationStatusInterrupted:
				text = "Subagent " + id + " was cancelled before it finished."
				notes.CancelledSubagents = []string{id}
			default:
				reason := "child ended with " + string(state.Run.Status)
				if state.Run.Error != nil {
					reason = state.Run.Error.Message
				}
				text = "Subagent " + id + " failed: " + reason
				notes.DeliveredSubagent = &id
			}
			record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), r.clock.Now())
			record.Provenance.Author = "user"
			record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Notes: notes}
			if err := r.save(x.ctx, x.inv.Session.ID, record); err != nil {
				return false, err
			}
			if err := r.consumeChild(x, child.session, &state.Run.Turn); err != nil {
				return false, err
			}
			return true, nil
		}
		if !pending {
			return false, nil
		}
		select {
		case <-x.ctx.Done():
			return false, x.ctx.Err()
		case <-changed:
		}
	}
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
		r.mu.Lock()
		for _, child := range x.foreground {
			current := child
			for current.successor != nil {
				current = current.successor
			}
			if child.session == session && turn != nil && current.turn == *turn {
				child.delivered = true
			}
		}
		r.mu.Unlock()
		r.publish()
		r.RefreshCommitted(x.inv.Session.ID)
	}
	return err
}

// stopChildren cancels every child of the turn's session, foreground or
// background, including those earlier turns left running. Children cannot
// delegate, so the session's children are its whole subtree. The turn keeps
// the children it cancelled: one that finishes and leaves the runtime before
// the turn settles them still has its completion record consumed.
func (r *Runtime) stopChildren(x *execution) {
	r.mu.Lock()
	for _, child := range r.children {
		if child.root == x.inv.Session.ID {
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
func (r *Runtime) cancelForeground(x *execution) {
	if x.ctx.Err() != nil {
		r.stopChildren(x)
	}
	r.mu.Lock()
	children := slices.Clone(x.foreground)
	r.mu.Unlock()
	var ids []string
	var stopped []*delegatedChild
	for _, child := range children {
		if !child.delivered {
			child.cancel()
			ids = append(ids, child.session.String())
			stopped = append(stopped, child)
		}
	}
	if len(ids) > 0 {
		text := ""
		for _, id := range ids {
			if text != "" {
				text += "\n"
			}
			text += "Subagent " + id + " was cancelled before it finished."
		}
		record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), r.clock.Now())
		record.Provenance.Author = "user"
		record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Notes: &canonical.OperationNotes{CancelledSubagents: ids}}
		_ = r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, record)
	}
	r.settleChildren(x, stopped)
	r.Wake()
}

// settleChildren waits for the session's cancelled children to finish, then
// consumes their completion records: the stopped work is never reported again
// with a later message or by a notice turn. Node shutdown settles children
// through its own path and is not waited for.
func (r *Runtime) settleChildren(x *execution, stopped []*delegatedChild) {
	ctx := context.WithoutCancel(r.ctx)
	for r.ctx.Err() == nil {
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
		running := false
		for _, child := range stopped {
			last := child.last()
			if r.children[last.turn] != last {
				continue
			}
			if last.started {
				running = true
			} else {
				queued = append(queued, last)
			}
		}
		r.mu.Unlock()
		if len(queued) > 0 {
			if r.finishQueued(ctx, queued) != nil {
				return
			}
			continue
		}
		if !running {
			break
		}
		select {
		case <-changed:
		case <-r.ctx.Done():
		}
	}
	if r.ctx.Err() != nil || len(stopped) == 0 {
		return
	}
	r.mu.Lock()
	turns := make([]protocol.TurnID, 0, len(stopped))
	for _, child := range stopped {
		turns = append(turns, child.last().turn)
	}
	r.mu.Unlock()
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(ctx, func(db *sql.Conn) error {
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
	r.mu.Lock()
	for _, child := range children {
		delete(r.children, child.turn)
	}
	r.mu.Unlock()
	r.publish()
	for _, child := range children {
		r.RefreshCommitted(child.session)
	}
	if len(children) > 0 {
		r.RefreshCommitted(children[0].root)
	}
	return nil
}
