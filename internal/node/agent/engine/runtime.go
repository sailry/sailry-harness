// Package engine owns ordered turn execution and its committed effects.
package engine

import (
	"context"
	"database/sql"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Options exposes the actual model and clock boundaries to deterministic tests.
type Options struct {
	Python            *python.Manager
	ValidateWorktree  func(context.Context, protocol.WorktreeID) error
	Attachment        func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error)
	AttachmentBytes   func(context.Context, protocol.Attachment) ([]byte, error)
	Clock             clock.Clock
	Model             func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	AuthorizeProvider func(context.Context, *store.AgentInvocation) error
	Tools             []Tool
	ToolsForTurn      func(context.Context, *store.AgentInvocation) ([]Tool, error)
	HooksForTurn      func(context.Context, *store.AgentInvocation) *hooks.Binding
	// AfterTurn, when set, runs after a main task turn's end is stored.
	AfterTurn func(context.Context, *store.AgentInvocation, protocol.ConversationStatus)
	Telemetry *telemetry.Control
	// Working, when set, brackets every running turn.
	Working func() func()
}

type Runtime struct {
	python            *python.Manager
	validateWorktree  func(context.Context, protocol.WorktreeID) error
	attachment        func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error)
	attachmentBytes   func(context.Context, protocol.Attachment) ([]byte, error)
	toolsForTurn      func(context.Context, *store.AgentInvocation) ([]Tool, error)
	hooksForTurn      func(context.Context, *store.AgentInvocation) *hooks.Binding
	afterTurn         func(context.Context, *store.AgentInvocation, protocol.ConversationStatus)
	telemetry         *telemetry.Control
	working           func() func()
	db                *store.Handle
	node              protocol.NodeID
	profile           string
	feed              *feed.Hub
	publish           func()
	clock             clock.Clock
	model             func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	authorizeProvider func(context.Context, *store.AgentInvocation) error
	tools             []Tool
	ctx               context.Context
	cancel            context.CancelFunc
	done              chan struct{}
	wake              chan struct{}
	mu                sync.Mutex
	active            map[protocol.TurnID]*execution
	children          map[protocol.TurnID]*delegatedChild
	activity          chan struct{}
	workers           sync.WaitGroup
	decisions         sync.Mutex
	publication       sync.Mutex
	statisticsMu      sync.Mutex
	statistics        map[protocol.SessionID]bool
	statisticsOwners  sync.WaitGroup
	statisticsStopped bool
}

type execution struct {
	python            []python.Access
	resources         *operations.Resources
	skillContext      string
	modeContext       string
	assistantContext  string
	childRole         *protocol.RoleProfile
	rolePolicy        string
	foreground        []*delegatedChild
	foregroundSteered bool
	renewedModel      fantasy.LanguageModel
	renewedSecret     protocol.Secret
	childBudget       uint64
	finishing         bool
	operations        uint64
	operationsDone    chan struct{}
	attachments       map[protocol.AttachmentID]*attachmentMaterial
	hooks             *hooks.Binding
	inv               *store.AgentInvocation
	ctx               context.Context
	cancel            context.CancelFunc
	done              chan struct{}
	wake              chan struct{}
	guidance          chan struct{}
	tools             []Tool
	toolCleanup       []func() error
	inputMu           sync.Mutex
	inputWake         chan struct{}
	output            output.Cache
}

func New(db *store.Handle, node protocol.NodeID, profile string, frames *feed.Hub, publish func(), options Options) *Runtime {
	if options.Clock == nil {
		options.Clock = clock.System{}
	}
	if options.Model == nil {
		options.Model = model.New
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Runtime{telemetry: options.Telemetry, working: options.Working, python: options.Python, validateWorktree: options.ValidateWorktree, attachment: options.Attachment, attachmentBytes: options.AttachmentBytes, db: db, node: node, profile: profile, feed: frames, publish: publish, clock: options.Clock, model: options.Model, authorizeProvider: options.AuthorizeProvider, tools: options.Tools, toolsForTurn: options.ToolsForTurn, hooksForTurn: options.HooksForTurn, afterTurn: options.AfterTurn, ctx: ctx, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1), active: map[protocol.TurnID]*execution{}, children: map[protocol.TurnID]*delegatedChild{}, activity: make(chan struct{}), statistics: map[protocol.SessionID]bool{}}
}

// Recover closes interrupted owners and holds every surviving queue. It never
// replays a model request or an external tool operation.
func (r *Runtime) Recover(ctx context.Context) error {
	unlock := r.LockPublication()
	defer unlock()
	err := r.db.Write(ctx, func(db *sql.Conn) error {
		changes, err := store.RecoverRuns(ctx, db, r.clock.Now())
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, r.node, changes)
		return err
	})
	if err == nil {
		r.publish()
	}
	return err
}

func (r *Runtime) Start() { go r.run() }
func (r *Runtime) Stop()  { r.cancel(); <-r.done }
func (r *Runtime) Wake() {
	r.mu.Lock()
	if r.activity != nil {
		close(r.activity)
	}
	r.activity = make(chan struct{})
	r.mu.Unlock()
	select {
	case r.wake <- struct{}{}:
	default:
	}
}

// LockPublication orders commits, their frames and pinned controller snapshots.
// Acquire decision ownership first; acquire storage leases only after this lock.
func (r *Runtime) LockPublication() func() {
	r.publication.Lock()
	return r.publication.Unlock
}

func (r *Runtime) run() {
	defer close(r.done)
	defer func() {
		r.workers.Wait()
		r.statisticsMu.Lock()
		r.statisticsStopped = true
		r.statisticsMu.Unlock()
		r.statisticsOwners.Wait()
	}()
	for {
		select {
		case <-r.ctx.Done():
			r.settleQueuedChildren()
			return
		case <-r.wake:
		}
		for r.ctx.Err() == nil {
			unlock := r.LockPublication()
			var inv *store.AgentInvocation
			changed := false
			err := r.db.Write(r.ctx, func(db *sql.Conn) error {
				var changes []protocol.Event
				var err error
				inv, changes, err = r.claimInvocation(r.ctx, db)
				changed = len(changes) > 0
				if err != nil {
					return err
				}
				_, err = store.RecordEvents(r.ctx, db, r.node, changes)
				return err
			})
			if err != nil || inv == nil {
				if err == nil && changed {
					r.publish()
					r.Wake()
				}
				unlock()
				break
			}
			r.publish()
			parent := r.ctx
			r.mu.Lock()
			if child := r.children[inv.Run.Turn]; child != nil {
				parent = child.ctx
				child.started = true
			}
			r.mu.Unlock()
			ctx, cancel := context.WithCancel(parent)
			x := &execution{inv: inv, ctx: ctx, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1), guidance: make(chan struct{}, 1)}
			// A turn has one owner. A claim that finds an earlier owner waits
			// for it in its worker instead of taking its place.
			r.mu.Lock()
			previous := r.active[inv.Run.Turn]
			if previous == nil {
				r.active[inv.Run.Turn] = x
			}
			r.mu.Unlock()
			// A stop may commit between claiming the durable turn and registering
			// its cancellation owner. Recheck after registration closes that gap.
			var current protocol.ConversationRun
			if previous == nil {
				err = r.db.Read(r.ctx, func(db *sql.Conn) error {
					var err error
					current, err = store.ReadAgentRun(r.ctx, db, inv.Run.Turn)
					return err
				})
				if err != nil || current.Status != protocol.ConversationStatusRunning {
					cancel()
				}
			}
			if previous == nil && err == nil {
				r.feed.Publish(inv.Session.ID, protocol.ConversationChange{Kind: "run", Data: current})
				r.RefreshStatisticsCommitted(inv.Session.ID)
				if inv.Session.Delegation != nil && *inv.Session.Delegation != nil {
					r.RefreshCommitted((*inv.Session.Delegation).Session)
				}
			}
			unlock()
			r.workers.Add(1)
			go func() {
				defer r.workers.Done()
				defer cancel()
				if previous == nil || r.succeed(x, previous) {
					r.executeOwned(x)
				}
				r.mu.Lock()
				delete(r.active, inv.Run.Turn)
				if child := r.children[inv.Run.Turn]; child != nil {
					child.cancel()
					delete(r.children, inv.Run.Turn)
				}
				r.mu.Unlock()
				close(x.done)
				r.Wake()
			}()
		}
	}
}

// succeed waits for the turn's earlier owners, then registers x in their
// place. It reports whether the turn is still x's to run: an owner that
// finished the turn after this claim leaves nothing to execute, and its input
// is never sent twice.
func (r *Runtime) succeed(x, previous *execution) bool {
	turn := x.inv.Run.Turn
	for previous != nil {
		<-previous.done
		r.mu.Lock()
		if previous = r.active[turn]; previous == nil {
			r.active[turn] = x
		}
		r.mu.Unlock()
	}
	unlock := r.LockPublication()
	defer unlock()
	var current protocol.ConversationRun
	err := r.db.Read(r.ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadAgentRun(r.ctx, db, turn)
		return err
	})
	if err != nil || current.Status == protocol.ConversationStatusStopping {
		x.cancel()
		return true
	}
	return current.Status == protocol.ConversationStatusRunning
}

// Committed wakes owners only after a controller transaction has committed.
// The durable queue remains the sole source of admitted work.
func (r *Runtime) Committed() {
	r.mu.Lock()
	owners := make([]*execution, 0, len(r.active))
	for _, x := range r.active {
		owners = append(owners, x)
	}
	r.mu.Unlock()
	for _, x := range owners {
		unlock := r.LockPublication()
		var run protocol.ConversationRun
		err := r.db.Read(r.ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(r.ctx, db, x.inv.Run.Turn)
			return err
		})
		if err == nil && run.Status == protocol.ConversationStatusStopping {
			r.feed.Seal(run.Session, run.Turn)
			r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
			x.cancel()
		}
		unlock()
		select {
		case x.wake <- struct{}{}:
		default:
		}
		x.inputMu.Lock()
		if x.inputWake != nil {
			close(x.inputWake)
			x.inputWake = make(chan struct{})
		}
		x.inputMu.Unlock()
		select {
		case x.guidance <- struct{}{}:
		default:
		}
	}
	r.Wake()
}

func (r *Runtime) Refresh(session protocol.SessionID) {
	unlock := r.LockPublication()
	defer unlock()
	r.RefreshCommitted(session)
}

// RefreshCommitted publishes a pinned metadata read while its caller owns
// publication, so no newer interaction or queue can overtake the read.
func (r *Runtime) RefreshCommitted(session protocol.SessionID) {
	var history protocol.ConversationHistory
	if err := r.db.Read(r.ctx, func(db *sql.Conn) error {
		var err error
		history, err = store.ReadConversation(r.ctx, db, session, nil, 20)
		return err
	}); err != nil {
		return
	}
	for _, run := range history.Page.Runs {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "run", Data: run})
	}
	r.feed.Publish(session, protocol.ConversationChange{Kind: "queue", Data: history.Page.Queue})
	for _, v := range history.Page.Approvals {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "approval", Data: v})
	}
	for _, task := range history.Page.Background {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "background", Data: task})
	}
	for _, child := range history.Page.Children {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "child", Data: child})
	}
	for _, v := range history.Page.Questions {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "question", Data: v})
	}
	r.RefreshStatisticsCommitted(session)
}

// OperationContext borrows a running turn's cancellation owner. The caller must
// finish its admitted effect before returning to the loop or closing the Node.
func (r *Runtime) OperationContext(turn protocol.TurnID) (context.Context, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	owner := r.active[turn]
	if owner == nil || owner.ctx.Err() != nil {
		return nil, false
	}
	return owner.ctx, true
}

func (r *Runtime) GuidanceSignal(turn protocol.TurnID) <-chan struct{} {
	r.mu.Lock()
	defer r.mu.Unlock()
	if x := r.active[turn]; x != nil {
		return x.guidance
	}
	return nil
}
