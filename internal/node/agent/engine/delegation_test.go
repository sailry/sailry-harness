package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
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
				f.waitRun(successor, protocol.ConversationStatusCancelled)
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
			returned <- f.r.admitChild(x, call.Arguments, false, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
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

// settled waits for a run to leave the active statuses, whatever its outcome.
func (f *engineFixture) settled(turn protocol.TurnID) protocol.ConversationRun {
	f.t.Helper()
	for {
		var run protocol.ConversationRun
		err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; run, err = store.ReadAgentRun(f.ctx, db, turn); return err })
		if err != nil {
			f.t.Fatal(err)
		}
		if !childActive(run.Status) {
			return run
		}
		select {
		case <-f.changed:
		case <-f.ctx.Done():
			f.t.Fatalf("run did not settle: %+v", run)
		}
	}
}

func (f *engineFixture) childTurns() []protocol.TurnID {
	f.t.Helper()
	var turns []protocol.TurnID
	err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		rows, err := db.QueryContext(f.ctx, "SELECT d.turn FROM agent_delegations d JOIN turns t ON t.id=d.turn ORDER BY t.rowid")
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var id string
			if err = rows.Scan(&id); err != nil {
				return err
			}
			turn, err := protocol.ParseTurnID(id)
			if err != nil {
				return err
			}
			turns = append(turns, turn)
		}
		return rows.Err()
	})
	if err != nil {
		f.t.Fatal(err)
	}
	return turns
}

func TestParentEndSettlesChildren(t *testing.T) {
	for _, ending := range []string{"user stop", "failed parent"} {
		t.Run(ending, func(t *testing.T) {
			childStarted, parentWaiting := make(chan struct{}), make(chan struct{})
			var childOnce, parentOnce sync.Once
			step := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
				prompt, results := "", 0
				for _, message := range request.Prompt {
					for _, part := range message.Content {
						if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
							prompt += value.Text + "\n"
						}
						if part.GetType() == fantasy.ContentTypeToolResult {
							results++
						}
					}
				}
				switch {
				case strings.Contains(prompt, "Follow-up"):
					_ = yield(text("Follow-up answer")) && yield(finish())
				case !strings.Contains(prompt, "Parent task"):
					childOnce.Do(func() { close(childStarted) })
					<-ctx.Done()
				case results == 0:
					_ = yield(call("first", "spawn_agent", `{"task":"First background child","background":true}`)) &&
						yield(call("second", "spawn_agent", `{"task":"Second background child","background":true}`)) && yield(finish())
				default:
					parentOnce.Do(func() { close(parentWaiting) })
					select {
					case <-childStarted:
					case <-ctx.Done():
						return
					}
					if ending == "failed parent" {
						yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: "fixture failure"}})
						return
					}
					<-ctx.Done()
				}
			}
			language := &scriptedModel{scripts: []script{step, step, step, step, step}}
			f := newFixture(t, language)
			// One slot keeps the second child queued while the first runs.
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				settings, err := store.ReadTeamSettings(f.ctx, db)
				if err != nil {
					return err
				}
				settings.ChildConcurrency = 1
				_, _, err = store.ExecuteTeamCommand(f.ctx, db, protocol.Command{Kind: "save_team_settings", Data: &settings})
				return err
			}); err != nil {
				t.Fatal(err)
			}
			f.start()
			parent := f.submit("Parent task")
			for _, signal := range []chan struct{}{childStarted, parentWaiting} {
				select {
				case <-signal:
				case <-f.ctx.Done():
					t.Fatal("parent and its background child did not both start")
				}
			}
			children := f.childTurns()
			if len(children) != 2 {
				t.Fatalf("admitted children=%d", len(children))
			}
			if ending == "user stop" {
				f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: parent})
			}
			ended := f.settled(parent)
			if ending == "failed parent" && ended.Status != protocol.ConversationStatusFailed {
				t.Fatalf("parent status=%s", ended.Status)
			}
			if ending == "user stop" && ended.Status == protocol.ConversationStatusCompleted {
				t.Fatal("stopped parent completed")
			}
			// The parent finished, so every child must already be terminal.
			for index, turn := range children {
				var run protocol.ConversationRun
				if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; run, err = store.ReadAgentRun(f.ctx, db, turn); return err }); err != nil {
					t.Fatal(err)
				}
				// A child stopped through its parent is cancelled, never a result.
				if run.Status != protocol.ConversationStatusCancelled {
					t.Fatalf("child %d outlived its parent or reported a result: %+v", index, run)
				}
				if run.FinishedMs == nil || ended.FinishedMs == nil || *run.FinishedMs > *ended.FinishedMs {
					t.Fatalf("parent finished before child %d settled: %+v", index, run)
				}
				if index == 1 && (run.StartedMs != nil || run.Status != protocol.ConversationStatusCancelled) {
					t.Fatalf("queued child was not cancelled unstarted: %+v", run)
				}
			}
			if len(f.r.children) != 0 {
				t.Fatal("cancelled children kept their owners")
			}
			if f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent' AND state!='consumed'") != 0 || f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent'") != 2 {
				t.Fatal("cancelled children left unreported completion records")
			}
			// New input reopens the queue: nothing the user stopped comes back.
			input := protocol.DefaultConversationInput()
			input.Text = "Follow-up"
			next := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: input}).Data.(protocol.QueuedTurn).ID
			if run := f.waitRun(next, protocol.ConversationStatusCompleted); len(run.Notices) != 0 {
				t.Fatalf("stopped children were delivered again: %+v", run.Notices)
			}
			if f.readQuery("SELECT count(*) FROM agent_runs WHERE coalesce(json_extract(body,'$.notice'),0)=1") != 0 {
				t.Fatal("a cancelled child started a notice turn")
			}
			if language.count() != 4 {
				t.Fatalf("model requests=%d", language.count())
			}
		})
	}
}

func TestStopSingleChild(t *testing.T) {
	childStarted := make(chan struct{})
	var once sync.Once
	step := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		prompt, results := "", 0
		for _, message := range request.Prompt {
			for _, part := range message.Content {
				if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
					prompt += value.Text + "\n"
				}
				if part.GetType() == fantasy.ContentTypeToolResult {
					results++
				}
			}
		}
		switch {
		case !strings.Contains(prompt, "Parent task"):
			once.Do(func() { close(childStarted) })
			<-ctx.Done()
		case results == 0:
			_ = yield(call("first", "spawn_agent", `{"task":"First child"}`)) && yield(call("second", "spawn_agent", `{"task":"Second child"}`)) && yield(finish())
		default:
			_ = yield(text("Parent continued")) && yield(finish())
		}
	}
	language := &scriptedModel{scripts: []script{step, step, step}}
	f := newFixture(t, language)
	// One slot keeps the second child queued while the first runs.
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		settings, err := store.ReadTeamSettings(f.ctx, db)
		if err != nil {
			return err
		}
		settings.ChildConcurrency = 1
		_, _, err = store.ExecuteTeamCommand(f.ctx, db, protocol.Command{Kind: "save_team_settings", Data: &settings})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	stopped := make(chan protocol.SessionID, 4)
	f.r.stopCommands = func(session protocol.SessionID) { stopped <- session }
	f.start()
	parent := f.submit("Parent task")
	select {
	case <-childStarted:
	case <-f.ctx.Done():
		t.Fatal("first child did not start")
	}
	children := f.childTurns()
	if len(children) != 2 {
		t.Fatalf("admitted children=%d", len(children))
	}
	read := func(turn protocol.TurnID) protocol.ConversationRun {
		t.Helper()
		var run protocol.ConversationRun
		if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; run, err = store.ReadAgentRun(f.ctx, db, turn); return err }); err != nil {
			t.Fatal(err)
		}
		return run
	}
	t.Run("a queued child is settled without starting", func(t *testing.T) {
		f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: children[1]})
		for {
			run := read(children[1])
			if run.FinishedMs != nil {
				if run.Status != protocol.ConversationStatusCancelled || run.StartedMs != nil {
					t.Fatalf("stopped queued child: %+v", run)
				}
				break
			}
			select {
			case <-f.changed:
			case <-f.ctx.Done():
				t.Fatalf("stopped queued child was never settled: %+v", run)
			}
		}
		if f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent' AND state='running'") != 1 {
			t.Fatal("stopped queued child left its completion record running")
		}
		if read(children[0]).Status != protocol.ConversationStatusRunning || read(parent).Status != protocol.ConversationStatusRunning {
			t.Fatal("stopping one child stopped its sibling or its parent")
		}
	})
	t.Run("a running child stops alone and the parent continues", func(t *testing.T) {
		f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: children[0]})
		if run := f.settled(children[0]); run.Status != protocol.ConversationStatusCancelled {
			t.Fatalf("stopped running child: %+v", run)
		}
		f.waitRun(parent, protocol.ConversationStatusCompleted)
		records := f.state().Records
		for _, turn := range children {
			mustHaveText(t, records, "Subagent "+read(turn).Session.String()+" was cancelled before it finished.")
		}
		mustHaveText(t, records, "Parent continued")
		if f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent' AND state!='consumed'") != 0 {
			t.Fatal("collected children left unreported completion records")
		}
		if language.count() != 3 {
			t.Fatalf("model requests=%d", language.count())
		}
		// Only the child that ran can own commands; they end with its run.
		select {
		case session := <-stopped:
			if session != read(children[0]).Session || len(stopped) != 0 {
				t.Fatalf("commands stopped for %s and %d more sessions", session, len(stopped))
			}
		case <-f.ctx.Done():
			t.Fatal("the ended child's commands were not stopped")
		}
	})
}

func TestChildWorkspacePreflight(t *testing.T) {
	// A package tool bound to agents.delegate or agents.guide sends its own
	// arguments; its host script prepares the ones that describe the child.
	record := func(f *engineFixture) *[]protocol.WorktreeID {
		var checked []protocol.WorktreeID
		f.r.validateWorktree = func(_ context.Context, id protocol.WorktreeID) error {
			checked = append(checked, id)
			return nil
		}
		return &checked
	}
	packaged := func(t *testing.T, f *engineFixture, x *execution, id string, arguments string) canonical.ToolRequest {
		t.Helper()
		call := canonical.ToolRequest{ID: id, Name: "spawn_agent", Arguments: json.RawMessage(arguments)}
		entry := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call}), f.r.clock.Now())
		entry.Provenance.Author = "assistant"
		if err := f.r.save(x.ctx, x.inv.Session.ID, entry); err != nil {
			t.Fatal(err)
		}
		return call
	}
	t.Run("the model's own fields do not refuse the child", func(t *testing.T) {
		f, x := delegationFixture(t)
		checked := record(f)
		call := packaged(t, f, x, "own schema", `{"session":"release branch","worktree":{"path":"src"}}`)
		prepared, _ := json.Marshal(map[string]any{"task": "Review the release branch"})
		result := f.r.spawnChild(x, call, prepared)
		if result.Failure != nil {
			t.Fatalf("prepared child was refused: %+v", result.Failure)
		}
		if len(*checked) != 1 || (*checked)[0] != x.inv.Run.Worktree {
			t.Fatalf("checked worktrees=%v", *checked)
		}
	})
	t.Run("the prepared worktree is the one checked", func(t *testing.T) {
		f, x := delegationFixture(t)
		checked := record(f)
		other := protocol.NewWorktreeID()
		call := packaged(t, f, x, "prepared worktree", `{"scope":"release"}`)
		prepared, _ := json.Marshal(map[string]any{"task": "Review the release branch", "worktree": other})
		// Admission then refuses the unregistered worktree; the preflight
		// must already have looked at it rather than at the parent's.
		f.r.spawnChild(x, call, prepared)
		if len(*checked) != 1 || (*checked)[0] != other {
			t.Fatalf("checked worktrees=%v, prepared %s", *checked, other)
		}
	})
	t.Run("a prepared session resolves to the child's worktree", func(t *testing.T) {
		f, x := delegationFixture(t)
		spawn := saveDelegationCall(t, f, x, "existing", true)
		if result := f.r.spawnChild(x, spawn, spawn.Arguments); result.Failure != nil {
			t.Fatal(result.Failure)
		}
		f.r.workers.Wait()
		var child *delegatedChild
		for _, value := range f.r.children {
			child = value
		}
		checked := record(f)
		call := canonical.ToolRequest{ID: "guide", Name: "guide_agent", Arguments: json.RawMessage(`{"session":"the reviewer","note":"continue"}`)}
		prepared, _ := json.Marshal(map[string]any{"session": child.session, "message": "Continue the review"})
		if err := f.r.preflightChildWorkspace(x.ctx, x, prepared); err != nil {
			t.Fatal(err)
		}
		if len(*checked) != 1 || (*checked)[0] != x.inv.Run.Worktree {
			t.Fatalf("checked worktrees=%v", *checked)
		}
		if err := f.r.preflightChildWorkspace(x.ctx, x, call.Arguments); err == nil {
			t.Fatal("the model's own guidance fields passed the workspace preflight")
		}
	})
}
