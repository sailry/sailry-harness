package engine

import (
	"database/sql"
	"encoding/json"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationPublication(t *testing.T) {
	for _, scenario := range []string{"commit_keeps_snapshot_consistent", "dirty_read_follows_entries", "bounded_admission"} {
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
					f.r.statistics[protocol.NewSessionID()] = false
				}
				f.r.RefreshStatistics(f.session.ID)
				if len(f.r.statistics) != statisticsCapacity || next().Change.Kind != "statistics_failed" {
					t.Fatal("saturated statistics owner admitted an unbounded read")
				}
				return
			}
			var stale protocol.ConversationStatistics
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				stale, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			f.r.statistics[f.session.ID] = false
			unlock := f.r.LockPublication()
			f.r.RequestStatisticsCommitted(f.session.ID)
			unlock()
			if f.r.statistics[f.session.ID] {
				t.Fatal("unchanged snapshot request dirtied an existing reader")
			}
			if err := f.r.save(f.ctx, f.session.ID, record); err != nil {
				t.Fatal(err)
			}
			if frame := next(); frame.Change.Kind != "entry" {
				t.Fatal("statistics overtook the committed entry")
			}
			before := f.r.feed.Sequence(f.session.ID)
			if f.r.finishStatistics(f.session.ID, stale, nil) || f.r.feed.Sequence(f.session.ID) != before {
				t.Fatal("dirty statistical snapshot was published")
			}
			var current protocol.ConversationStatistics
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				current, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if !f.r.finishStatistics(f.session.ID, current, nil) || current.Responses != 1 || next().Change.Kind != "statistics" {
				t.Fatal("fresh aggregate did not follow its canonical entry")
			}
			if len(f.r.statistics) != 0 {
				t.Fatal("completed statistics owner remained retained")
			}
		})
	}
}
