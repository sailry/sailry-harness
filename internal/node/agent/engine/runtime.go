// Package engine owns ordered turn execution and its committed effects.
package engine

import (
	"context"
	"database/sql"
	"errors"
	"log/slog"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Options exposes the actual model and clock boundaries to deterministic tests.
type Options struct {
	Python             *python.Manager
	ValidateWorktree   func(context.Context, protocol.WorktreeID) error
	Attachment         func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error)
	AttachmentBytes    func(context.Context, protocol.Attachment) ([]byte, error)
	Clock              clock.Clock
	Model              func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	AuthorizeProvider  func(context.Context, *store.AgentInvocation) error
	Tools              []Tool
	ToolsForTurn       func(context.Context, *store.AgentInvocation) ([]Tool, error)
	HooksForTurn       func(context.Context, *store.AgentInvocation) *hooks.Binding
	PluginAvailability func(protocol.PluginReference) error
	ToolPresentation   store.ToolPresentationLookup
	NativeSkills       []operations.NativeSkillPackage
	// AfterTurn, when set, runs after a main task turn's end is stored. It
	// receives the settled run and what the execution observed.
	AfterTurn func(context.Context, *store.AgentInvocation, protocol.ConversationRun, TurnObservation)
	Telemetry *telemetry.Control
	// Working, when set, brackets every running turn.
	Working func() func()
	// StopCommands, when set, stops a session's background commands. The
	// engine calls it for a child session whose run has ended.
	StopCommands func(protocol.SessionID)
	// StopTransient, when set, stops a session's non-service background
	// commands. The engine calls it once a durable stop of the session's
	// running turn commits: a user stop, Send now or a disabled package.
	StopTransient func(protocol.SessionID)
	// MaxModelSteps bounds how many main-turn model inference steps a single
	// turn may execute before halting cleanly with a turn-limit note. Zero uses
	// defaultMaxMainModelSteps; a negative value disables the limit.
	MaxModelSteps int
}

const defaultMaxMainModelSteps uint32 = 200

type Runtime struct {
	python            *python.Manager
	validateWorktree  func(context.Context, protocol.WorktreeID) error
	attachment        func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error)
	attachmentBytes   func(context.Context, protocol.Attachment) ([]byte, error)
	toolsForTurn      func(context.Context, *store.AgentInvocation) ([]Tool, error)
	hooksForTurn      func(context.Context, *store.AgentInvocation) *hooks.Binding
	afterTurn         func(context.Context, *store.AgentInvocation, protocol.ConversationRun, TurnObservation)
	telemetry         *telemetry.Control
	working           func() func()
	stopCommands      func(protocol.SessionID)
	stopTransient     func(protocol.SessionID)
	maxModelSteps     uint32
	nativeSkills      []operations.NativeSkillPackage
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
	responsesHistory  observation.ResponsesHistory
	statisticsMu      sync.Mutex
	statistics        map[protocol.SessionID]statisticsFlight
	statisticsHistory []statisticsCompleted
	statisticsChanges statisticsDependencies
	statisticsOwners  sync.WaitGroup
	statisticsStopped bool
}

// TurnObservation is what one execution saw that storage does not keep apart:
// the tool calls the model made, by the name it called, and the approvals and
// questions that expired unanswered.
type TurnObservation struct {
	Tools   map[string]int
	Expired int
}

type execution struct {
	observed          TurnObservation
	observedMu        sync.Mutex
	python            []python.Access
	resources         *operations.Resources
	skillContext      string
	modeContext       string
	assistantContext  string
	childRole         *protocol.RoleProfile
	rolePolicy        string
	foreground        []*delegatedChild
	foregroundSteered bool
	callbackPending   bool
	stopped           []*delegatedChild // the session's children this turn cancelled; guarded by Runtime.mu
	renewedModel      fantasy.LanguageModel
	renewedSecret     protocol.Secret
	// sessionRoute: the model keeps the session's history and receives only
	// new messages, so rewriting earlier ones would replay all of it.
	sessionRoute    bool
	childBudget     uint64
	finishing       bool
	commandsStopped bool                    // guarded by Runtime.publication
	queuedSteer     []protocol.TurnID       // guarded by Runtime.publication
	directSteer     bool                    // guarded by Runtime.publication
	interruptTools  context.CancelCauseFunc // guarded by Runtime.publication
	interruptStream context.CancelCauseFunc // guarded by Runtime.publication
	streamReads     *streamedReads
	operations      uint64
	operationsDone  chan struct{}
	attachments     map[protocol.AttachmentID]*attachmentMaterial
	hooks           *hooks.Binding
	timeline        *timeline
	inv             *store.AgentInvocation
	ctx             context.Context
	cancel          context.CancelFunc
	done            chan struct{}
	wake            chan struct{}
	guidance        chan struct{}
	tools           []Tool
	parentTools     inheritedTools
	delegationTools inheritedTools
	names           *exposure
	toolCleanup     []func() error
	inputMu         sync.Mutex
	inputWake       chan struct{}
	output          output.Cache
	modelSteps      uint32
	doomSignature   string
	doomStreak      int
}

func New(db *store.Handle, node protocol.NodeID, profile string, frames *feed.Hub, publish func(), options Options) *Runtime {
	if options.Clock == nil {
		options.Clock = clock.System{}
	}
	if options.Model == nil {
		options.Model = model.New
	}
	var maxSteps uint32
	switch {
	case options.MaxModelSteps > 0:
		maxSteps = uint32(options.MaxModelSteps)
	case options.MaxModelSteps == 0:
		maxSteps = defaultMaxMainModelSteps
	}
	base := store.WithPluginAvailability(context.Background(), options.PluginAvailability)
	base = store.WithToolPresentation(base, options.ToolPresentation)
	ctx, cancel := context.WithCancel(base)
	return &Runtime{telemetry: options.Telemetry, working: options.Working, stopCommands: options.StopCommands, stopTransient: options.StopTransient, maxModelSteps: maxSteps, nativeSkills: options.NativeSkills, python: options.Python, validateWorktree: options.ValidateWorktree, attachment: options.Attachment, attachmentBytes: options.AttachmentBytes, db: db, node: node, profile: profile, feed: frames, publish: publish, clock: options.Clock, model: options.Model, authorizeProvider: options.AuthorizeProvider, tools: options.Tools, toolsForTurn: options.ToolsForTurn, hooksForTurn: options.HooksForTurn, afterTurn: options.AfterTurn, ctx: ctx, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1), active: map[protocol.TurnID]*execution{}, children: map[protocol.TurnID]*delegatedChild{}, activity: make(chan struct{}), statistics: map[protocol.SessionID]statisticsFlight{}}
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

// BeginStop interrupts turns before the Node cancels their dependencies. Stop
// joins their settlement after the command lanes have released their locks.
func (r *Runtime) BeginStop() { r.cancel() }
func (r *Runtime) Stop()      { r.BeginStop(); <-r.done }
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

// ActivitySignal returns a channel closed at the next wake, which follows
// every settled execution. Take it before reading the state it guards.
func (r *Runtime) ActivitySignal() <-chan struct{} { return r.activitySignal() }

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
		retries := 0
		for r.ctx.Err() == nil {
			unlock := r.LockPublication()
			var inv *store.AgentInvocation
			var ended []*delegatedChild
			var changes []protocol.Event
			err := r.db.Write(r.ctx, func(db *sql.Conn) error {
				var err error
				inv, changes, ended, err = r.claimInvocation(r.ctx, db)
				if err != nil {
					return err
				}
				_, err = store.RecordEvents(r.ctx, db, r.node, changes)
				return err
			})
			if err != nil {
				unlock()
				if retries == 0 && r.ctx.Err() == nil {
					logging.Emit(slog.LevelError, slog.String("area", "agent"), slog.String("phase", logging.Failed),
						slog.String("operation", "claim"), slog.String("exception", err.Error()))
				}
				failure, ok := errors.AsType[*protocol.Fault](err)
				// The writer can report an uncertain COMMIT. Retry only when
				// storage says it did not admit this claim.
				if !ok || failure.Code != protocol.ErrorCodeBusy && failure.Code != protocol.ErrorCodeUnavailable || retries == 3 || r.clock.Sleep(r.ctx, time.Second) != nil {
					break
				}
				retries++
				continue
			}
			retries = 0
			changedChildren := make(map[protocol.SessionID][]protocol.TurnID)
			for _, child := range ended {
				changedChildren[child.root] = append(changedChildren[child.root], child.turn)
				r.releaseQueuedChild(child)
			}
			if inv != nil || len(changes) > 0 {
				r.publish()
			}
			// A capture can settle in the same pass that starts another
			// session. Open feeds need that settlement as well as the event.
			refreshed := map[protocol.SessionID]bool{}
			for _, event := range changes {
				value, ok := event.Data.(protocol.EventConversationChangedPayload)
				if ok && !refreshed[value.Session] && (inv == nil || value.Session != inv.Session.ID) {
					refreshed[value.Session] = true
					r.RefreshCommitted(value.Session, changedChildren[value.Session]...)
					delete(changedChildren, value.Session)
				}
			}
			for parent, turns := range changedChildren {
				r.RefreshCommitted(parent, turns...)
			}
			if inv == nil {
				if len(changes) > 0 {
					r.Wake()
				}
				unlock()
				break
			}
			parent := r.ctx
			var parentTools inheritedTools
			r.mu.Lock()
			if child := r.children[inv.Run.Turn]; child != nil {
				parent = child.ctx
				parentTools = child.tools
				inv.ConnectionTargets = child.connections
				child.started = true
			}
			r.mu.Unlock()
			ctx, cancel := context.WithCancel(store.WithCapturedUser(store.WithUser(parent, inv.User), inv.StorageUser))
			ctx, trace := r.responsesHistory.Start(ctx, inv.Session.ID, inv.Run.Turn)
			x := &execution{inv: inv, ctx: ctx, cancel: cancel, done: make(chan struct{}), wake: make(chan struct{}, 1), guidance: make(chan struct{}, 1), parentTools: parentTools}
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
					r.RefreshCommitted((*inv.Session.Delegation).Session, inv.Run.Turn)
				}
			}
			unlock()
			r.workers.Add(1)
			go func() {
				defer r.workers.Done()
				defer cancel()
				defer trace.Clear()
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

// StopCommitted delivers a durable Stop before projecting the changed history.
// The caller holds the publication lock, so the turn cannot finish or be
// replaced between this decision and cancellation of its owned work.
func (r *Runtime) StopCommitted(run protocol.ConversationRun) {
	if run.Status != protocol.ConversationStatusStopping {
		return
	}
	r.mu.Lock()
	x := r.active[run.Turn]
	r.mu.Unlock()
	if x != nil && x.ctx.Err() == nil {
		x.cancel()
		r.stopChildren(x)
		r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
	}
	r.stopTurnCommands(run.Session, x)
}

// stopTurnCommands ends a session's transient command work once for the turn.
// Call under publication, before a successor can claim the session.
func (r *Runtime) stopTurnCommands(session protocol.SessionID, x *execution) {
	if r.stopTransient == nil || x != nil && x.commandsStopped {
		return
	}
	if x != nil {
		x.commandsStopped = true
	}
	r.stopTransient(session)
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
		if err == nil {
			r.StopCommitted(run)
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
func (r *Runtime) RefreshCommitted(session protocol.SessionID, changedChildren ...protocol.TurnID) {
	// Run transitions maintain the active draft feed independently. A later
	// observer reserves and reads current durable state under publication.
	if !r.feed.Observed(session) {
		if len(changedChildren) > 0 {
			r.refreshStatisticsForTurnsCommitted(session, changedChildren)
		}
		return
	}
	var activity store.ConversationActivity
	if err := r.db.Read(r.ctx, func(db *sql.Conn) error {
		var err error
		activity, err = store.ReadConversationActivity(r.ctx, db, session, 20, changedChildren...)
		return err
	}); err != nil {
		return
	}
	for _, run := range activity.Runs {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "run", Data: run})
	}
	r.feed.Publish(session, protocol.ConversationChange{Kind: "queue", Data: activity.Queue})
	for _, v := range activity.Approvals {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "approval", Data: v})
	}
	r.feed.PublishBackground(session, activity.Background)
	for _, child := range activity.Children {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "child", Data: child})
	}
	for _, v := range activity.Questions {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "question", Data: v})
	}
	r.refreshStatisticsForTurnsCommitted(session, changedChildren)
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
