package engine

import (
	"context"
	"database/sql"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestForegroundChildCompletionHookSettlement(t *testing.T) {
	for _, row := range []struct {
		name, end     string
		exitCode      int32
		refuseReceipt bool
		status        protocol.ConversationStatus
		code          protocol.ErrorCode
		diagnostic    string
	}{
		{name: "parent ends", status: protocol.ConversationStatusCancelled},
		{name: "script fails as parent ends", exitCode: 7, status: protocol.ConversationStatusFailed, code: protocol.ErrorCodeUnavailable, diagnostic: "hook failed: exited 7"},
		{name: "receipt fails as parent ends", refuseReceipt: true, status: protocol.ConversationStatusFailed, code: protocol.ErrorCodeOutcomeUnknown, diagnostic: "hook completion could not be recorded"},
		{name: "Node shuts down", end: "shutdown", status: protocol.ConversationStatusInterrupted},
		{name: "explicit Stop preserves uncertain receipt", end: "stop", refuseReceipt: true, status: protocol.ConversationStatusCancelled, code: protocol.ErrorCodeOutcomeUnknown, diagnostic: "hook completion could not be recorded"},
	} {
		t.Run(row.name, func(t *testing.T) {
			language := &scriptedModel{scripts: []script{response(text("Child response"), finish())}}
			f, parent := delegationFixture(t, language)
			settings := protocol.DefaultHooksSettings()
			settings.Enabled = true
			settings.TurnEnd = &protocol.HooksScript{Enabled: true, Command: "fixture", TimeoutMs: 1000}
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				if _, err := store.SaveHooks(f.ctx, db, settings, settings.Revision); err != nil {
					return err
				}
				if row.refuseReceipt {
					_, err := db.ExecContext(f.ctx, `CREATE TRIGGER refuse_hook_receipt BEFORE UPDATE OF result ON hook_executions
						BEGIN SELECT RAISE(ABORT,'injected hook receipt failure'); END`)
					return err
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			spawn := saveDelegationCall(t, f, parent, "foreground", false)
			if result := f.r.spawnChild(parent, spawn, spawn.Arguments); result.Failure != nil {
				t.Fatal(result.Failure)
			}
			f.r.workers.Wait()
			child := parent.foreground[0]
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				_, err := store.ReleaseChildren(f.ctx, db, parent.inv.Run.Turn, []protocol.TurnID{child.turn})
				return err
			}); err != nil {
				t.Fatal(err)
			}
			child.released = true

			entered := make(chan string, 1)
			outcome := protocol.ProcessOutcome{Kind: "cancelled"}
			if row.exitCode != 0 {
				outcome = protocol.ProcessOutcome{Kind: "exited", Data: row.exitCode}
			}
			f.r.hooksForTurn = func(ctx context.Context, inv *store.AgentInvocation) *hooks.Binding {
				return hooks.New(hooks.Capture{Admitted: true, Turn: inv.Run.Turn, Stop: ctx, Settings: inv.Hooks,
					Claim: func(ctx context.Context, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64) error {
						return f.db.Node.Write(ctx, func(db *sql.Conn) error { return store.ClaimHook(ctx, db, turn, phase, ordinal) })
					},
					Finish: func(ctx context.Context, turn protocol.TurnID, phase protocol.HooksPhase, ordinal uint64, outcome protocol.ProcessOutcome, err error) error {
						return f.db.Node.Write(ctx, func(db *sql.Conn) error { return store.FinishHook(ctx, db, turn, phase, ordinal, outcome, err) })
					},
					Launch: func(ctx context.Context, _ string, _ uint64, env map[string]string, _ string) (protocol.ProcessCompletion, error) {
						entered <- env["SAILRY_HOOK_STATUS"]
						select {
						case <-ctx.Done():
							return protocol.ProcessCompletion{Outcome: outcome}, nil
						case <-f.ctx.Done():
							return protocol.ProcessCompletion{}, f.ctx.Err()
						}
					},
				})
			}
			f.start()
			select {
			case status := <-entered:
				if status != "completed" {
					t.Fatalf("hook started with status %q", status)
				}
			case <-f.ctx.Done():
				t.Fatal("child did not reach its completion hook")
			}
			switch row.end {
			case "shutdown":
				f.r.cancel()
			case "stop":
				f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: child.turn})
			default:
				// The parent's end cancels the child owner without storing a Stop.
				settled := make(chan bool, 1)
				f.r.workers.Go(func() { settled <- f.r.cancelForeground(parent) })
				select {
				case ok := <-settled:
					if !ok {
						t.Fatal("parent did not settle its foreground child")
					}
				case <-f.ctx.Done():
					t.Fatal("parent did not leave foreground settlement")
				}
			}
			run := f.settled(child.turn)
			if run.Status != row.status || run.FinishedMs == nil {
				t.Fatalf("child settled as %s (%v), want %s", run.Status, run.Error, row.status)
			}
			if row.code == "" {
				if run.Error != nil {
					t.Fatalf("child fault = %v, want none", run.Error)
				}
			} else if run.Error == nil || run.Error.Code != row.code || !strings.Contains(run.Error.Message, row.diagnostic) {
				t.Fatalf("child fault = %v, want code %q and diagnostic %q", run.Error, row.code, row.diagnostic)
			}
			var written bool
			var receipt sql.NullString
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				return db.QueryRowContext(f.ctx, `SELECT result IS NOT NULL, json_extract(result,'$.Ok.kind')
					FROM hook_executions WHERE turn=?1 AND phase='turn_end' AND ordinal=0`, child.turn.String()).Scan(&written, &receipt)
			}); err != nil {
				t.Fatal(err)
			}
			if row.refuseReceipt {
				if written {
					t.Fatal("the refused hook receipt was persisted")
				}
			} else if !written || receipt.String != outcome.Kind {
				t.Fatalf("hook receipt = %v, want %s", receipt, outcome.Kind)
			}
		})
	}
}

func TestParentSettlementRetriesChildWrites(t *testing.T) {
	for _, row := range []struct {
		name, boundary string
		refuseFinish   bool
		shutdown       bool
	}{
		{name: "late stop in hook waits for queued child", boundary: "hook", refuseFinish: true},
		{name: "late stop in cleanup waits for consumption", boundary: "cleanup"},
		{name: "ordinary failed parent waits for consumption"},
		{name: "shutdown stops waiting without replay", boundary: "hook", refuseFinish: true, shutdown: true},
	} {
		t.Run(row.name, func(t *testing.T) {
			lateStop := row.boundary != ""
			step := response(text("Parent response"), finish())
			if !lateStop {
				step = response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: "fixture failure"}})
			}
			language := &scriptedModel{scripts: []script{step}}
			// Don't start the dispatcher: the parent's direct settlement must own
			// the queued child's end, not the claim pass.
			f, parent := delegationFixture(t, language)
			spawn := saveDelegationCall(t, f, parent, "background", true)
			result := f.r.spawnChild(parent, spawn, spawn.Arguments)
			if result.Failure != nil {
				t.Fatal(result.Failure)
			}
			f.r.workers.Wait()
			receipt := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, spawn, result)), f.r.clock.Now())
			receipt.Provenance.Author = "tool"
			if err := f.r.save(parent.ctx, parent.inv.Session.ID, receipt); err != nil {
				t.Fatal(err)
			}
			children := f.childTurns()
			if len(children) != 1 || f.r.children[children[0]] == nil {
				t.Fatalf("missing queued owner: %v", children)
			}
			child := f.r.children[children[0]]
			// A queued successor can already own its predecessor's commands.
			child.commandsInherited = lateStop
			stopped := make(chan protocol.SessionID, 4)
			f.r.stopCommands = func(session protocol.SessionID) { stopped <- session }

			entered, resume := make(chan struct{}), make(chan struct{})
			pause := func() error {
				close(entered)
				select {
				case <-resume:
					return nil
				case <-f.ctx.Done():
					return f.ctx.Err()
				}
			}
			var closes, hooksRun atomic.Int32
			probe := objectTool("probe", true, true, nil)
			probe.Close = func() error {
				if closes.Add(1) == 1 && row.boundary == "cleanup" {
					return pause()
				}
				return nil
			}
			f.r.tools = []Tool{probe}
			f.r.hooksForTurn = func(ctx context.Context, inv *store.AgentInvocation) *hooks.Binding {
				return hooks.New(hooks.Capture{
					Admitted: true,
					Turn:     inv.Run.Turn,
					Stop:     ctx,
					Settings: protocol.HooksSettings{Enabled: true, TurnEnd: &protocol.HooksScript{Enabled: true, Command: "fixture", TimeoutMs: 1000}},
					Claim:    func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error { return nil },
					Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
						return nil
					},
					Launch: func(context.Context, string, uint64, map[string]string, string) (protocol.ProcessCompletion, error) {
						if hooksRun.Add(1) == 1 && row.boundary == "hook" {
							if err := pause(); err != nil {
								return protocol.ProcessCompletion{}, err
							}
						}
						return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
					},
				})
			}

			write := func(query string) {
				t.Helper()
				if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
					_, err := db.ExecContext(f.ctx, query)
					return err
				}); err != nil {
					t.Fatal(err)
				}
			}
			if row.refuseFinish {
				write(`CREATE TRIGGER refuse_child_settlement BEFORE UPDATE OF body ON agent_runs
					WHEN NEW.turn = '` + child.turn.String() + `' AND json_extract(NEW.body,'$.finished_ms') IS NOT NULL
					BEGIN SELECT RAISE(ABORT,'injected child finish failure'); END`)
			} else {
				write(`CREATE TRIGGER refuse_child_settlement BEFORE UPDATE OF state ON agent_background
					WHEN OLD.kind='agent' AND NEW.state='consumed'
					BEGIN SELECT RAISE(ABORT,'injected child consumption failure'); END`)
			}
			done := make(chan struct{})
			f.r.workers.Go(func() {
				defer close(done)
				f.r.executeOwned(parent)
			})
			parentTurn := parent.inv.Run.Turn
			activeStatus, finalStatus := protocol.ConversationStatusRunning, protocol.ConversationStatusFailed
			if lateStop {
				select {
				case <-entered:
				case <-done:
					t.Fatal("parent ended before the completion boundary")
				case <-f.ctx.Done():
					t.Fatal("parent did not reach the completion boundary")
				}
				// Commit Stop without delivering the runtime wake. The owner must
				// see the stored Stop after it leaves the hook or cleanup.
				unlock := f.r.LockPublication()
				err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
					request := protocol.NewRequest(f.node, protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: parentTurn}})
					_, _, err := store.ExecuteAgentCommand(f.ctx, db, f.node, f.r.profile, f.node, request, f.r.clock.Now())
					return err
				})
				unlock()
				if err != nil {
					t.Fatal(err)
				}
				close(resume)
				activeStatus, finalStatus = protocol.ConversationStatusStopping, protocol.ConversationStatusCancelled
			}

			var wait clockWait
			select {
			case wait = <-f.r.clock.(*manualClock).waits:
				if wait.duration != settlementRetry {
					t.Fatalf("settlement retry delay = %s", wait.duration)
				}
			case <-done:
				t.Fatal("parent ended despite the child write failure")
			case <-f.ctx.Done():
				t.Fatal("parent did not retry the child write")
			}
			readRun := func(turn protocol.TurnID) protocol.ConversationRun {
				t.Helper()
				var run protocol.ConversationRun
				if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
					var err error
					run, err = store.ReadAgentRun(f.ctx, db, turn)
					return err
				}); err != nil {
					t.Fatal(err)
				}
				return run
			}
			if run := readRun(parentTurn); run.Status != activeStatus || run.FinishedMs != nil {
				t.Fatalf("parent settled before the child write: %+v", run)
			}
			if row.refuseFinish && (readRun(child.turn).FinishedMs != nil || len(stopped) != 0) {
				t.Fatal("an uncommitted child finish stopped commands or became terminal")
			}
			if row.shutdown {
				f.r.cancel()
			} else {
				write("DROP TRIGGER refuse_child_settlement")
				close(wait.advance)
			}
			select {
			case <-done:
			case <-f.ctx.Done():
				t.Fatal("parent did not leave settlement")
			}
			if language.count() != 1 || closes.Load() != 1 || hooksRun.Load() != 1 {
				t.Fatalf("settlement work counts: model=%d, cleanup=%d, hook=%d; want one each", language.count(), closes.Load(), hooksRun.Load())
			}
			ended := readRun(parentTurn)
			if row.shutdown {
				if ended.Status != activeStatus || ended.FinishedMs != nil || len(stopped) != 0 {
					t.Fatalf("shutdown advanced unconfirmed settlement: parent=%+v, commands stopped=%d", ended, len(stopped))
				}
				return
			}
			cancelled := readRun(child.turn)
			if ended.Status != finalStatus || ended.FinishedMs == nil || cancelled.Status != protocol.ConversationStatusCancelled || cancelled.StartedMs != nil || cancelled.FinishedMs == nil || *cancelled.FinishedMs > *ended.FinishedMs {
				t.Fatalf("parent or queued child settled out of order: parent=%+v, child=%+v", ended, cancelled)
			}
			if f.readQuery("SELECT count(*) FROM agent_background WHERE kind='agent' AND state='consumed'") != 1 {
				t.Fatal("the cancelled child's result was not consumed")
			}
			if child.commandsInherited {
				select {
				case session := <-stopped:
					if session != child.session {
						t.Fatalf("stopped commands for %s, want %s", session, child.session)
					}
				default:
					t.Fatal("the directly settled successor kept its inherited commands")
				}
			}
			if len(stopped) != 0 {
				t.Fatal("settlement stopped commands more than once or for a child without inherited commands")
			}
		})
	}
}
