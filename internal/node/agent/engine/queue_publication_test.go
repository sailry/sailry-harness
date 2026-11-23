package engine

import (
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueuePublicationAfterImmediateStop(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name       string
		claimed    bool
		recorded   bool
		nextBefore bool
	}{
		{name: "stop before claim"},
		{name: "stop before input", claimed: true},
		{name: "stop after input", claimed: true, recorded: true},
		{name: "next submission before settlement", claimed: true, nextBefore: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			model := &scriptedModel{}
			f := newFixture(t, model)
			t.Cleanup(func() {
				f.r.cancel()
				f.r.statisticsMu.Lock()
				f.r.statisticsStopped = true
				f.r.statisticsMu.Unlock()
				f.r.statisticsOwners.Wait()
			})
			unlock := f.r.LockPublication()
			reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			t.Cleanup(func() { _ = reservation.Close() })
			var snapshot protocol.ConversationSnapshot
			err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				snapshot, err = store.ReadConversationSnapshot(f.ctx, db, f.session.ID, 20, reservation.Drafts)
				return err
			})
			stream := reservation.Subscribe(snapshot)
			unlock()
			if err != nil {
				t.Fatal(err)
			}
			projection := client.NewConversationProjection(f.node, f.session.ID, 1)
			initial, err := stream.Next(f.ctx)
			if err != nil {
				t.Fatal(err)
			}
			if got, err := projection.Apply(1, initial); got != client.Applied || err != nil {
				t.Fatalf("initial snapshot: %v, %v", got, err)
			}
			drain := func() protocol.ConversationQueue {
				t.Helper()
				through := f.r.feed.Sequence(f.session.ID)
				for projection.Snapshot().Sequence < through {
					update, err := stream.Next(f.ctx)
					if err != nil {
						t.Fatal(err)
					}
					if got, err := projection.Apply(1, update); got != client.Applied || err != nil {
						t.Fatalf("published queue transition was not reducible: %+v: %v, %v", update, got, err)
					}
					before := projection.Snapshot()
					if got, err := projection.Apply(1, update); got != client.Ignored || err != nil || !reflect.DeepEqual(before, projection.Snapshot()) {
						t.Fatalf("duplicate publication changed state: %v, %v", got, err)
					}
				}
				return projection.Snapshot().Page.Queue
			}
			write := func(change func(*sql.Conn) error) {
				t.Helper()
				unlock := f.r.LockPublication()
				defer unlock()
				if err := f.db.Node.Write(f.ctx, change); err != nil {
					t.Fatal(err)
				}
				f.r.RefreshCommitted(f.session.ID)
			}
			execute := func(request protocol.Request) protocol.Output {
				t.Helper()
				body, err := json.Marshal(request)
				if err != nil {
					t.Fatal(err)
				}
				decision := f.r.LockDecision(request.Command)
				defer decision()
				var result link.Response
				write(func(db *sql.Conn) error {
					previous, err := admission.Admit(f.ctx, db, f.node, request, body)
					if err != nil {
						return err
					}
					if previous != nil {
						result = *previous
						return nil
					}
					output, events, err := store.ExecuteAgentCommand(f.ctx, db, f.node, f.r.profile, f.node, request, f.r.clock.Now())
					if err != nil {
						return err
					}
					if _, err = store.RecordEvents(f.ctx, db, f.node, events); err != nil {
						return err
					}
					result.Output = output
					return admission.Complete(f.ctx, db, f.node, request.ID, result)
				})
				if result.Fault != nil || result.Output.Kind == "" {
					t.Fatalf("command outcome: %+v", result)
				}
				return result.Output
			}
			submit := func(text string) protocol.Request {
				return protocol.NewRequest(f.node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
					Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: protocol.ConversationInput{Text: text},
				}})
			}
			claim := func(turn protocol.TurnID) {
				t.Helper()
				write(func(db *sql.Conn) error {
					invocation, _, err := store.ClaimNext(f.ctx, db, f.r.clock.Now())
					if err != nil {
						return err
					}
					if invocation == nil || invocation.Run.Turn != turn {
						return errors.New("claim did not select the submitted input")
					}
					return nil
				})
				drain()
			}
			finish := func(turn protocol.TurnID, status protocol.ConversationStatus) {
				t.Helper()
				write(func(db *sql.Conn) error {
					_, _, err := store.FinishTurn(f.ctx, db, turn, status, nil, f.r.clock.Now())
					return err
				})
			}
			save := func(turn protocol.TurnID, text string) {
				t.Helper()
				if err := f.r.save(f.ctx, f.session.ID, inputRecord(turn, protocol.ConversationInput{Text: text}, f.r)); err != nil {
					t.Fatal(err)
				}
				drain()
			}

			request := submit("First input")
			first := execute(request).Data.(protocol.QueuedTurn).ID
			queued := drain()
			if len(queued.Items) != 1 || queued.Items[0].Turn != first {
				t.Fatalf("submission was not published: %+v", queued)
			}
			execute(request)
			if replayed := drain(); !reflect.DeepEqual(replayed, queued) {
				t.Fatalf("replaying the same request admitted another input: %+v", replayed)
			}
			if test.claimed {
				claim(first)
			}
			if test.recorded {
				save(first, "First input")
			}
			execute(protocol.NewRequest(f.node, protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: first}}))
			stopped := drain()
			if len(stopped.Items) != 0 {
				t.Fatalf("stopping input remained visible: %+v", stopped)
			}
			var next protocol.TurnID
			if test.nextBefore {
				next = execute(submit("Next input")).Data.(protocol.QueuedTurn).ID
				stopped = drain()
			}
			if test.claimed {
				finish(first, protocol.ConversationStatusCancelled)
				settled := drain()
				if settled.Revision <= stopped.Revision || settled.Paused != stopped.Paused {
					t.Fatalf("settlement published an unversioned queue or undid resume: before %+v, after %+v", stopped, settled)
				}
			}
			if !test.nextBefore {
				next = execute(submit("Next input")).Data.(protocol.QueuedTurn).ID
				drain()
			}
			claim(next)
			save(next, "Next input")
			finish(next, protocol.ConversationStatusCompleted)
			final := drain()
			if test.claimed && !test.recorded {
				if len(final.Items) != 1 || final.Items[0].Turn != first || final.Items[0].Ready {
					t.Fatalf("retained input was lost or rearmed: %+v", final)
				}
			} else if len(final.Items) != 0 {
				t.Fatalf("consumed or removed input was replayed: %+v", final)
			}
			if model.count() != 0 {
				t.Fatal("publication fixture executed a model")
			}
		})
	}
}
