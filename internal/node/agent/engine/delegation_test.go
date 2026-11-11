package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func delegationFixture(t *testing.T, models ...*scriptedModel) (*engineFixture, *execution) {
	t.Helper()
	language := &scriptedModel{}
	if len(models) > 0 {
		language = models[0]
	}
	f := newFixture(t, language)
	turn := f.submit("Delegate bounded work")
	var inv *store.AgentInvocation
	err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		var err error
		inv, _, err = store.ClaimNext(f.ctx, db, f.r.clock.Now())
		return err
	})
	if err != nil || inv == nil || inv.Run.Turn != turn {
		t.Fatal(inv, err)
	}
	if err := core.PrivateDirectory(inv.Worktree.Path); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(f.ctx)
	x := &execution{inv: inv, ctx: ctx, cancel: cancel, wake: make(chan struct{}, 1), guidance: make(chan struct{}, 1)}
	f.r.active[turn] = x
	t.Cleanup(func() { cancel(); f.r.cancel(); f.r.workers.Wait() })
	if err := f.r.save(ctx, inv.Session.ID, inputRecord(turn, inv.Message, f.r)); err != nil {
		t.Fatal(err)
	}
	return f, x
}

func saveDelegationCall(t *testing.T, f *engineFixture, x *execution, id string, background bool) canonical.ToolRequest {
	t.Helper()
	args, _ := json.Marshal(map[string]any{"task": "Inspect the captured resource", "background": background})
	call := canonical.ToolRequest{ID: id, Name: "spawn_agent", Arguments: args}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call}), f.r.clock.Now())
	record.Provenance.Author = "assistant"
	if err := f.r.save(x.ctx, x.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	return call
}

func TestWaitAgent(t *testing.T) {
	for _, guidance := range []bool{false, true} {
		name := "bounded observation keeps queued child"
		if guidance {
			name = "actual main guidance returns early"
		}
		t.Run(name, func(t *testing.T) {
			f, x := delegationFixture(t)
			spawn := saveDelegationCall(t, f, x, "background", true)
			result := f.r.spawnChild(x, spawn, spawn.Arguments)
			if result.Failure != nil {
				t.Fatal(result.Failure)
			}
			f.r.workers.Wait()
			var child *delegatedChild
			for _, value := range f.r.children {
				child = value
			}
			if child == nil {
				t.Fatal("no admitted child owner")
			}
			args, _ := json.Marshal(map[string]any{"session": child.session})
			returned := make(chan ToolResult, 1)
			go func() {
				returned <- f.r.waitChild(x, canonical.ToolRequest{ID: "wait", Name: "wait_agent", Arguments: args})
			}()
			clock := f.r.clock.(*manualClock)
			var wait clockWait
			select {
			case wait = <-clock.waits:
			case <-f.ctx.Done():
				t.Fatal(f.ctx.Err())
			}
			if wait.duration != 150*time.Second {
				t.Fatal("observation window", wait.duration)
			}
			f.r.Wake()
			select {
			case <-returned:
				t.Fatal("invalidation was treated as guidance or completion")
			default:
			}
			if guidance {
				f.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: x.inv.Run.Turn, ExpectedRevision: x.inv.Run.Revision, Message: protocol.ConversationInput{Text: "Apply my new direction"}})
			} else {
				close(wait.advance)
			}
			select {
			case result = <-returned:
			case <-f.ctx.Done():
				t.Fatal(f.ctx.Err())
			}
			value, ok := result.Value.(map[string]any)
			if !ok || result.Failure != nil || value["session"] != child.session || value["turn"] != child.turn {
				t.Fatal(result)
			}
			if guidance {
				if value["guidance_pending"] != true {
					t.Fatal(value)
				}
			} else if value["admitted"] != true || value["pending"] != true {
				t.Fatal(value)
			}
			if child.ctx.Err() != nil {
				t.Fatal("observation stopped durable child work")
			}
		})
	}
}

func TestDelegationHandoff(t *testing.T) {
	for _, outcome := range []string{"completed", "failed recipient", "cancelled successor"} {
		t.Run("final boundary guidance "+outcome, func(t *testing.T) {
			successorStarted := make(chan struct{})
			language := &scriptedModel{scripts: []script{
				response(text("Initial child response"), finish()),
				func(ctx context.Context, input fantasy.Call, yield func(fantasy.StreamPart) bool) {
					close(successorStarted)
					if outcome == "cancelled successor" {
						<-ctx.Done()
						return
					}
					response(text("Final successor response"), finish())(ctx, input, yield)
				},
			}}
			f, parent := delegationFixture(t, language)
			spawn := saveDelegationCall(t, f, parent, "late guidance", false)
			result := f.r.spawnChild(parent, spawn, spawn.Arguments)
			if result.Failure != nil {
				t.Fatal(result.Failure)
			}
			f.r.workers.Wait()
			child := parent.foreground[0]
			batch := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, spawn, result)), f.r.clock.Now())
			batch.Provenance.Author = "tool"
			if err := f.r.save(parent.ctx, parent.inv.Session.ID, batch); err != nil {
				t.Fatal(err)
			}
			boundary := make(chan protocol.ConversationRun, 1)
			release := make(chan struct{})
			var released sync.Once
			unblock := func() { released.Do(func() { close(release) }) }
			f.r.hooksForTurn = func(_ context.Context, inv *store.AgentInvocation) *hooks.Binding {
				if inv.Run.Turn != child.turn {
					return nil
				}
				return hooks.New(hooks.Capture{
					Admitted: true,
					Turn:     inv.Run.Turn,
					Settings: protocol.HooksSettings{Enabled: true, TurnEnd: &protocol.HooksScript{Enabled: true, Command: "fixture", TimeoutMs: 1000}},
					Claim:    func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error { return nil },
					Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
						return nil
					},
					Launch: func(context.Context, string, uint64, map[string]string) (protocol.ProcessCompletion, error) {
						boundary <- inv.Run
						select {
						case <-release:
						case <-f.ctx.Done():
							return protocol.ProcessCompletion{}, f.ctx.Err()
						}
						if outcome == "failed recipient" {
							return protocol.ProcessCompletion{}, errors.New("fixture turn completion failed")
						}
						return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
					},
				})
			}
			f.start()
			t.Cleanup(func() { unblock(); parent.cancel() })
			collected := make(chan error, 1)
			go func() {
				delivered, err := f.r.foregroundStep(parent)
				if err == nil && !delivered {
					err = errors.New("foreground child ended without a result")
				}
				collected <- err
			}()
			var recipient protocol.ConversationRun
			select {
			case recipient = <-boundary:
			case <-f.ctx.Done():
				t.Fatal(f.ctx.Err())
			}
			args, _ := json.Marshal(map[string]any{"session": child.session, "message": "First final-boundary input"})
			guide := canonical.ToolRequest{ID: "late guide", Name: "guide_agent", Arguments: args}
			record := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &guide}), f.r.clock.Now())
			record.Provenance.Author = "assistant"
			if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
				t.Fatal(err)
			}
			receipt := f.r.guideChild(parent, guide, guide.Arguments)
			value, ok := receipt.Value.(map[string]any)
			if !ok || receipt.Failure != nil || value["recipient"] != recipient.Turn || value["session"] != child.session || value["turn"] == recipient.Turn {
				t.Fatalf("running guidance receipt: %+v", receipt)
			}
			successor, ok := value["turn"].(protocol.TurnID)
			if !ok {
				t.Fatal("guidance receipt lost its queued identity")
			}
			record = canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, guide, receipt)), f.r.clock.Now())
			record.Provenance.Author = "tool"
			if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
				t.Fatal(err)
			}
			followup := f.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: recipient.Turn, ExpectedRevision: recipient.Revision, Message: protocol.ConversationInput{Text: "Second final-boundary input"}}).Data.(protocol.QueuedTurn)
			if language.count() != 1 {
				t.Fatal("guidance began while its recipient was still running")
			}
			unblock()
			if outcome == "cancelled successor" {
				select {
				case <-successorStarted:
				case <-f.ctx.Done():
					t.Fatal(f.ctx.Err())
				}
				parent.cancel()
				f.waitRun(successor, protocol.ConversationStatusInterrupted)
			}
			select {
			case err := <-collected:
				if outcome == "cancelled successor" {
					if !errors.Is(err, context.Canceled) {
						t.Fatal("foreground cancellation", err)
					}
				} else if err != nil {
					t.Fatal(err)
				}
			case <-f.ctx.Done():
				t.Fatal(f.ctx.Err())
			}
			if outcome == "failed recipient" {
				f.waitRun(recipient.Turn, protocol.ConversationStatusFailed)
				if language.count() != 1 {
					t.Fatal("failed recipient restarted queued guidance")
				}
				var queue protocol.ConversationQueue
				if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
					var err error
					queue, err = store.ReadQueue(f.ctx, db, child.session)
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if !queue.Paused || len(queue.Items) != 2 || queue.Items[0].Turn != successor || queue.Items[1].Turn != followup.ID {
					t.Fatalf("failed guidance was not retained: %+v", queue)
				}
				if f.readQuery("SELECT count(*) FROM agent_pending WHERE recipient IS NOT NULL") != 0 {
					t.Fatal("failed owner retained guidance targets")
				}
				return
			}
			if outcome == "completed" {
				finished := f.waitRun(successor, protocol.ConversationStatusCompleted)
				if finished.Session != child.session || finished.Worktree != recipient.Worktree {
					t.Fatal("successor changed the admitted child scope")
				}
				mustHaveText(t, f.state().Records, "Subagent "+child.session.String()+" completed: Final successor response")
			}
			joined := f.waitRun(followup.ID, protocol.ConversationStatusCompleted)
			if joined.Joined == nil || *joined.Joined != successor || language.count() != 2 {
				t.Fatal("same-boundary guidance did not share one successor")
			}
			state, err := f.r.load(f.ctx, child.session)
			if err != nil {
				t.Fatal(err)
			}
			mustHaveText(t, state.Records, "First final-boundary input")
			mustHaveText(t, state.Records, "Second final-boundary input")
			if f.readQuery("SELECT count(*) FROM agent_pending") != 0 {
				t.Fatal("successful guidance input remained queued")
			}
		})
	}
	t.Run("detached background releases parent drain", func(t *testing.T) {
		f, x := delegationFixture(t)
		call := saveDelegationCall(t, f, x, "detached", true)
		result := f.r.spawnChild(x, call, call.Arguments)
		if result.Failure != nil {
			t.Fatal(result.Failure)
		}
		f.r.drainOperations(x)
		x.cancel()
		for _, child := range f.r.children {
			if child.ctx.Err() != nil {
				t.Fatal("background child retained the parent lifetime")
			}
		}
		if f.readQuery("SELECT count(*) FROM agent_delegations") != 1 {
			t.Fatal("child admission did not remain durable")
		}
	})
	t.Run("cancelled tool retains exactly one committed handoff", func(t *testing.T) {
		f, x := delegationFixture(t)
		call := saveDelegationCall(t, f, x, "cancelled handoff", false)
		entered, release := make(chan struct{}), make(chan struct{})
		var releaseOnce sync.Once
		unblock := func() { releaseOnce.Do(func() { close(release) }) }
		t.Cleanup(unblock)
		returned := make(chan ToolResult, 1)
		go func() {
			returned <- f.r.admitChild(x, call, false, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
				inv, events, err := store.DelegateAgent(ctx, db, f.node, x.inv.Run.Turn, toolIdentity(call), nil, false, f.r.clock.Now())
				if err != nil {
					return nil, nil, err
				}
				close(entered)
				<-release
				return &inv, events, nil
			})
		}()
		select {
		case <-entered:
		case <-f.ctx.Done():
			t.Fatal(f.ctx.Err())
		}
		x.cancel()
		result := <-returned
		if result.Failure == nil || result.Failure.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatal(result)
		}
		unblock()
		f.r.workers.Wait()
		if f.readQuery("SELECT count(*) FROM agent_delegations") != 1 {
			t.Fatal("cancelled receipt lost durable admission")
		}
		for _, child := range f.r.children {
			if child.ctx.Err() == nil {
				t.Fatal("foreground handoff ignored parent cancellation")
			}
		}
	})
}
