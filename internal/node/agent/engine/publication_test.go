package engine

import (
	"database/sql"
	"encoding/json"
	"errors"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationPublication(t *testing.T) {
	for _, scenario := range []string{"commit_keeps_snapshot_consistent", "dirty_read_follows_entries", "resubscribe_fences_unobserved_commit", "bounded_admission", "unobserved_statistics", "unobserved_refresh"} {
		t.Run(scenario, func(t *testing.T) {
			f := newFixture(t, &scriptedModel{})
			t.Cleanup(func() {
				f.r.cancel()
				f.r.statisticsMu.Lock()
				f.r.statisticsStopped = true
				f.r.statisticsMu.Unlock()
				f.r.statisticsOwners.Wait()
			})
			turn := f.submit("Task")
			var invocation *store.AgentInvocation
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				var err error
				invocation, _, err = store.ClaimNext(f.ctx, db, f.r.clock.Now())
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if invocation == nil || invocation.Run.Turn != turn {
				t.Fatal("publication fixture did not claim its turn")
			}
			f.r.feed.Publish(f.session.ID, protocol.ConversationChange{Kind: "run", Data: invocation.Run})
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Committed answer"))
			input, output := uint64(10), uint64(1)
			message.Usage = &canonical.Usage{Input: &input, Output: &output}
			record := canonical.NewRecord(turn, "message", &message, f.r.clock.Now())
			record.Provenance.Author = "assistant"
			if scenario == "unobserved_refresh" {
				if err := f.r.save(f.ctx, f.session.ID, record); err != nil {
					t.Fatal(err)
				}
				before := f.r.feed.Sequence(f.session.ID)
				f.r.Refresh(f.session.ID)
				if f.r.feed.Sequence(f.session.ID) != before {
					t.Fatal("metadata refresh sent frames without an observer")
				}
				unlock := f.r.LockPublication()
				reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
				defer reservation.Close()
				var history protocol.ConversationHistory
				err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
					var err error
					history, err = store.ReadConversation(f.ctx, db, f.session.ID, nil, 20)
					return err
				})
				unlock()
				if err != nil || len(history.Page.Runs) != 1 || len(history.Page.Entries) != 1 || history.Page.Entries[0].ID != record.ID {
					t.Fatalf("new observer did not find unobserved activity in durable history: %v", err)
				}
				f.r.Refresh(f.session.ID)
				if f.r.feed.Sequence(f.session.ID) <= before {
					t.Fatal("metadata refresh omitted a reserved observer")
				}
				return
			}
			if scenario == "unobserved_statistics" {
				if err := f.r.save(f.ctx, f.session.ID, record); err != nil {
					t.Fatal(err)
				}
				// Hold publication so an erroneously admitted statistics reader cannot finish.
				unlock := f.r.LockPublication()
				f.r.RefreshStatisticsCommitted(f.session.ID)
				_, admitted := f.r.statistics[f.session.ID]
				unlock()
				if admitted {
					t.Fatal("statistics reader admitted without a subscriber")
				}
				reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
				defer reservation.Close()
				unlock = f.r.LockPublication()
				f.r.RequestStatisticsCommitted(f.session.ID)
				unlock()
				stream := reservation.Subscribe(protocol.DefaultConversationSnapshot())
				if _, err := stream.Next(f.ctx); err != nil {
					t.Fatal(err)
				}
				update, err := stream.Next(f.ctx)
				if err != nil {
					t.Fatal(err)
				}
				frame := update.Data.(*protocol.ConversationFrame)
				if frame.Change.Kind != "statistics" || frame.Change.Data.(*protocol.ConversationStatistics).Responses != 1 {
					t.Fatal("new subscriber missed statistics from an unobserved commit")
				}
				f.r.statisticsMu.Lock()
				cached := len(f.r.statisticsHistory) == 1 && f.r.statisticsHistory[0].session == f.session.ID
				f.r.statisticsMu.Unlock()
				if !cached {
					t.Fatal("completed statistics were not retained")
				}
				_ = reservation.Close()
				second := canonical.NewRecord(turn, "message", &message, f.r.clock.Now())
				second.Provenance.Author = "assistant"
				if err := f.r.save(f.ctx, f.session.ID, second); err != nil {
					t.Fatal(err)
				}
				unlock = f.r.LockPublication()
				reservation = f.r.feed.Reserve(f.session.ID, make(chan struct{}))
				defer reservation.Close()
				f.r.RequestStatisticsCommitted(f.session.ID)
				stream = reservation.Subscribe(protocol.DefaultConversationSnapshot())
				unlock()
				if _, err := stream.Next(f.ctx); err != nil {
					t.Fatal(err)
				}
				if update, err := stream.Next(f.ctx); err != nil {
					t.Fatal(err)
				} else if frame := update.Data.(*protocol.ConversationFrame); frame.Change.Kind != "statistics" || frame.Change.Data.(*protocol.ConversationStatistics).Responses != 2 {
					t.Fatalf("resubscription published stale completed statistics: %+v", frame)
				}
				return
			}
			if scenario == "commit_keeps_snapshot_consistent" {
				if err := f.r.feed.Delta(f.session.ID, protocol.ConversationDraft{ID: record.ID, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Partial"}}}); err != nil {
					t.Fatal(err)
				}
				committed, release := make(chan struct{}), make(chan struct{})
				var once sync.Once
				resume := func() { once.Do(func() { close(release) }) }
				defer resume()
				f.r.publish = func() { close(committed); <-release }
				saved := make(chan error, 1)
				go func() { saved <- f.r.save(f.ctx, f.session.ID, record) }()
				select {
				case <-committed:
				case <-f.ctx.Done():
					t.Fatal("save did not reach committed publication")
				}
				if f.r.publication.TryLock() {
					f.r.publication.Unlock()
					t.Fatal("commit released publication before publishing its canonical frames")
				}
				type captured struct {
					value protocol.ConversationSnapshot
					err   error
				}
				result := make(chan captured, 1)
				started := make(chan struct{})
				go func() {
					close(started)
					unlock := f.r.LockPublication()
					defer unlock()
					reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
					defer reservation.Close()
					value := protocol.DefaultConversationSnapshot()
					err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
						history, err := store.ReadConversation(f.ctx, db, f.session.ID, nil, 20)
						value.Page, value.Missing = history.Page, history.Missing
						return err
					})
					if err == nil {
						update, readErr := reservation.Subscribe(value).Next(f.ctx)
						err = readErr
						if err == nil {
							body, _ := json.Marshal(update.Data)
							err = json.Unmarshal(body, &value)
						}
					}
					result <- captured{value, err}
				}()
				<-started
				resume()
				if err := <-saved; err != nil {
					t.Fatal(err)
				}
				var snapshot protocol.ConversationSnapshot
				select {
				case got := <-result:
					if got.err != nil {
						t.Fatal(got.err)
					}
					snapshot = got.value
				case <-f.ctx.Done():
					t.Fatal("snapshot did not follow committed publication")
				}
				if len(snapshot.Drafts) != 0 || len(snapshot.Page.Entries) != 1 || snapshot.Page.Entries[0].ID != record.ID {
					t.Fatal("snapshot mixed a finalized entry with an earlier streaming draft")
				}
				projection := client.NewConversationProjection(f.node, f.session.ID, 1)
				if applied, err := projection.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: snapshot}); err != nil || applied != client.Applied {
					t.Fatalf("coherent snapshot was not reducible: %v %v", applied, err)
				}
				return
			}
			reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			defer reservation.Close()
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page.Session = f.session.ID
			stream := reservation.Subscribe(snapshot)
			if _, err := stream.Next(f.ctx); err != nil {
				t.Fatal(err)
			}
			next := func() protocol.ConversationFrame {
				t.Helper()
				update, err := stream.Next(f.ctx)
				if err != nil {
					t.Fatal(err)
				}
				var frame protocol.ConversationFrame
				body, err := json.Marshal(update.Data)
				if err != nil || json.Unmarshal(body, &frame) != nil {
					t.Fatal("conversation frame expected")
				}
				return frame
			}
			if scenario == "bounded_admission" {
				for range statisticsCapacity {
					f.r.statistics[protocol.NewSessionID()] = statisticsFlight{}
				}
				f.r.RefreshStatistics(f.session.ID)
				if len(f.r.statistics) != statisticsCapacity || next().Change.Kind != "statistics_failed" {
					t.Fatal("saturated statistics owner admitted an unbounded read")
				}
				return
			}
			stale := statisticsCompleted{session: f.session.ID}
			var old protocol.ConversationStatistics
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				old, stale.history, err = store.ReadAgentStatisticsReusing(f.ctx, db, f.session.ID, nil)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			stale.weight = stale.history.Weight()
			f.r.beginStatisticsRead(f.session.ID)
			if scenario == "resubscribe_fences_unobserved_commit" {
				_ = reservation.Close()
				if f.r.feed.Observed(f.session.ID) {
					t.Fatal("an observer survived the disconnect")
				}
				// A commit made without an observer need not dispatch a refresh.
				unlock := f.r.LockPublication()
				err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
					_, _, err := store.AppendCanonical(f.ctx, db, f.session.ID, record)
					return err
				})
				if err != nil {
					unlock()
					t.Fatal(err)
				}
				reservation = f.r.feed.Reserve(f.session.ID, make(chan struct{}))
				defer reservation.Close()
				err = f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
					history, err := store.ReadConversation(f.ctx, db, f.session.ID, nil, 20)
					snapshot.Page, snapshot.Missing = history.Page, history.Missing
					return err
				})
				if err != nil {
					unlock()
					t.Fatal(err)
				}
				f.r.RequestStatisticsCommitted(f.session.ID)
				stream = reservation.Subscribe(snapshot)
				unlock()
				if update, err := stream.Next(f.ctx); err != nil || update.Kind != "conversation_snapshot" || len(snapshot.Page.Entries) != 1 || snapshot.Page.Entries[0].ID != record.ID {
					t.Fatalf("new reservation did not see the unobserved commit: %v", err)
				}
			} else {
				if err := f.r.save(f.ctx, f.session.ID, record); err != nil {
					t.Fatal(err)
				}
				if frame := next(); frame.Change.Kind != "entry" {
					t.Fatal("statistics overtook the committed entry")
				}
			}
			before := f.r.feed.Sequence(f.session.ID)
			if f.r.finishStatistics(stale, old, nil) || f.r.feed.Sequence(f.session.ID) != before || len(f.r.statisticsHistory) != 0 {
				t.Fatal("dirty statistical snapshot was published")
			}
			f.r.beginStatisticsRead(f.session.ID)
			var current protocol.ConversationStatistics
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				current, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if !f.r.finishStatistics(statisticsCompleted{session: f.session.ID}, current, nil) || current.Responses != 1 || next().Change.Kind != "statistics" {
				t.Fatal("fresh aggregate did not follow its canonical entry")
			}
			if len(f.r.statistics) != 0 {
				t.Fatal("completed statistics owner remained retained")
			}
		})
	}
}

func TestConversationStatisticsChildAppend(t *testing.T) {
	f := newFixture(t, &scriptedModel{})
	t.Cleanup(func() {
		f.r.cancel()
		f.r.statisticsMu.Lock()
		f.r.statisticsStopped = true
		f.r.statisticsMu.Unlock()
		f.r.statisticsOwners.Wait()
	})
	parent := f.submit("Delegate work")
	var child *store.AgentInvocation
	var unrelated protocol.SessionID
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		now := f.r.clock.Now()
		invocation, _, err := store.ClaimNext(f.ctx, db, now)
		if err != nil {
			return err
		}
		if invocation == nil || invocation.Run.Turn != parent {
			return errors.New("parent was not claimed")
		}
		call := canonical.ToolRequest{ID: "delegate", Name: "spawn_agent", Arguments: json.RawMessage(`{"task":"Child task","background":true}`)}
		record := canonical.NewRecord(parent, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call}), now)
		record.Provenance.Author = "assistant"
		if _, _, err := store.AppendCanonical(f.ctx, db, f.session.ID, record); err != nil {
			return err
		}
		queued, _, err := store.DelegateAgent(f.ctx, db, f.node, parent, canonical.ToolIdentity{ID: &call.ID, Name: call.Name, Arguments: call.Arguments}, nil, true, now)
		if err != nil {
			return err
		}
		child, _, err = store.ClaimChild(f.ctx, db, queued.Run.Turn, now)
		if err != nil {
			return err
		}
		if child == nil {
			return errors.New("child was not claimed")
		}
		if _, _, err := store.FinishTurn(f.ctx, db, parent, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		request := protocol.NewRequest(f.node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &f.session.Config}})
		output, _, err := store.ExecuteCommand(f.ctx, db, f.node, f.node, f.r.profile, request, store.PreparedPaths{})
		if err == nil {
			unrelated = output.Data.(protocol.Session).ID
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	fork := f.command("fork_conversation", &protocol.CommandForkConversationPayload{Session: f.session.ID, Through: parent, ExpectedRevision: f.session.Revision}).Data.(protocol.Session)
	observe := func(session protocol.SessionID, requestStatistics bool) link.Subscription {
		t.Helper()
		unlock := f.r.LockPublication()
		reservation := f.r.feed.Reserve(session, make(chan struct{}))
		t.Cleanup(func() { _ = reservation.Close() })
		if requestStatistics {
			f.r.RequestStatisticsCommitted(session)
		}
		snapshot := protocol.DefaultConversationSnapshot()
		snapshot.Page.Session = session
		stream := reservation.Subscribe(snapshot)
		unlock()
		if _, err := stream.Next(f.ctx); err != nil {
			t.Fatal(err)
		}
		return stream
	}
	parentStream, forkStream := observe(f.session.ID, true), observe(fork.ID, true)
	observe(unrelated, false)
	next := func(stream link.Subscription, want uint64) protocol.ConversationStatistics {
		t.Helper()
		update, err := stream.Next(f.ctx)
		if err != nil {
			t.Fatal(err)
		}
		frame, ok := update.Data.(*protocol.ConversationFrame)
		if !ok || frame.Change.Kind != "statistics" {
			t.Fatalf("statistics frame expected, got %+v", update)
		}
		result := frame.Change.Data.(*protocol.ConversationStatistics)
		if result.Responses != 0 || result.Delegated.Responses != want || result.Observations.Children != 1 {
			t.Fatalf("session %s statistics = %+v, want %d delegated responses", frame.Session, result, want)
		}
		return *result
	}
	old := next(parentStream, 0)
	next(forkStream, 0)
	// Leave a pre-commit parent read in flight until the dependency resolves.
	f.r.beginStatisticsRead(f.session.ID)
	parentBefore, unrelatedBefore := f.r.feed.Sequence(f.session.ID), f.r.feed.Sequence(unrelated)

	// Hold every storage lease after the write so the dependency read cannot
	// run, while the commit and its publication are free to finish.
	type blockedStorage struct {
		lease *store.Exclusive
		err   error
	}
	blocked := make(chan blockedStorage)
	f.r.publish = func() {
		lease, err := f.db.Node.Exclusive(f.ctx)
		select {
		case blocked <- blockedStorage{lease, err}:
		case <-f.ctx.Done():
			if lease != nil {
				_ = lease.Close()
			}
		}
	}
	message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Child response"))
	input, output := uint64(30), uint64(3)
	message.Usage = &canonical.Usage{Input: &input, Output: &output}
	record := canonical.NewRecord(child.Run.Turn, "message", &message, f.r.clock.Now())
	record.Provenance.Author = "assistant"
	saved := make(chan error, 1)
	go func() { saved <- f.r.save(f.ctx, child.Session.ID, record) }()
	var lease *store.Exclusive
	select {
	case value := <-blocked:
		if value.err != nil {
			t.Fatal(value.err)
		}
		lease = value.lease
		defer lease.Close()
	case err := <-saved:
		t.Fatalf("save missed its committed publication: %v", err)
	case <-f.ctx.Done():
		t.Fatal("save did not reach committed publication")
	}
	select {
	case err := <-saved:
		if err != nil {
			t.Fatal(err)
		}
	case <-f.ctx.Done():
		t.Fatal("save waited for its blocked dependency lookup")
	}
	stale := statisticsCompleted{session: f.session.ID}
	if f.r.finishStatistics(stale, old, nil) || f.r.feed.Sequence(f.session.ID) != parentBefore {
		t.Fatal("a pre-commit dependent statistics snapshot published before dependency resolution")
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	next(forkStream, 1)
	if f.r.finishStatistics(stale, old, nil) || f.r.feed.Sequence(f.session.ID) != parentBefore {
		t.Fatal("dependency resolution did not dirty the parent's in-flight snapshot")
	}
	f.r.beginStatisticsRead(f.session.ID)
	var current protocol.ConversationStatistics
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if !f.r.finishStatistics(statisticsCompleted{session: f.session.ID}, current, nil) {
		t.Fatal("the refreshed parent statistics did not publish")
	}
	next(parentStream, 1)
	f.r.statisticsMu.Lock()
	_, pending := f.r.statistics[unrelated]
	f.r.statisticsMu.Unlock()
	if pending || f.r.feed.Sequence(unrelated) != unrelatedBefore {
		t.Fatal("the dependency lookup refreshed an unrelated conversation")
	}
	if f.r.feed.Observed(child.Session.ID) {
		t.Fatal("test unexpectedly opened a child subscription")
	}
}
