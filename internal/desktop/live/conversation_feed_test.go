package live

import (
	"fmt"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fragmentedSnapshot(node protocol.NodeID, id protocol.SessionID, chunks int) protocol.ConversationSnapshot {
	s := protocol.DefaultConversationSnapshot()
	s.Node, s.Sequence, s.Page = node, 10, cacheTestPage(id, 1, id.String())
	s.Page.Entries = s.Page.Entries[:1]
	for i := range chunks {
		s.Page.Entries = append(s.Page.Entries, protocol.ConversationEntry{ID: fmt.Sprintf("reply/%d", i), Turn: s.Page.Runs[0].Turn, Sequence: uint64(i + 2), Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat("x", 128)}}})
	}
	return s
}

// Standalone view tests have no event loop to redraw on the worker's wake.
func awaitConversationFeed(t *testing.T, r *Runtime, id protocol.SessionID) {
	t.Helper()
	wake := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	defer r.SetInvalidate(nil)
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	for {
		feed := r.conversationFeeds[id]
		if feed == nil {
			t.Fatal("conversation feed is not mounted")
		}
		r.takeConversationFeed(id, feed)
		if r.conversationCache[id].state.Connected {
			return
		}
		select {
		case <-wake:
		case <-deadline.C:
			t.Fatal("no prepared conversation publication")
		}
	}
}

func TestConversationSwitchDoesNotWaitForPreparation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		first, second := protocol.SessionID{1}, protocol.SessionID{2}
		node := protocol.DefaultSnapshot()
		node.Node = protocol.NodeID{3}
		source := &liveWorkspaceBenchmarkTransport{conversations: map[protocol.SessionID]protocol.ConversationSnapshot{}}
		for _, id := range []protocol.SessionID{first, second} {
			s := protocol.DefaultSession()
			s.ID, s.Revision = id, 1
			node.Sessions = append(node.Sessions, s)
			source.conversations[id] = fragmentedSnapshot(node.Node, id, 1024)
		}
		source.snapshot = node
		controller := client.New(source)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		r.conversationLane = make(chan struct{}, 1)
		r.conversationLane <- struct{}{}
		selected := first
		tester := ui.NewTester(func(c *ui.Context) {
			r.beginConversationFrame()
			if selected != (protocol.SessionID{}) {
				cv := r.App.Data.Find(selected.String())
				r.conversation(c, cv, r.App.Composer(cv.ID))
			}
			r.finishFrame()
			r.finishConversationFrame()
		}, 640, 400)
		tester.SetLocale("en")
		tester.Frame()
		synctest.Wait()
		// The Client has the whole range; only Desktop preparation is held.
		if s, _ := controller.Conversation(first); !s.Connected || len(s.Snapshot.Page.Entries) != 1025 {
			t.Fatal("first source was not loaded")
		}
		old := r.conversationFeeds[first]
		selected = second
		tester.Frame()
		synctest.Wait()
		cv := r.App.Data.Find(second.String())
		if cv == nil || !cv.Session.Loading || len(cv.Turns) != 0 || r.conversationFeeds[first] != nil || old.latest.Load() != nil {
			t.Fatal("switch waited for or reused the previous preparation")
		}
		<-r.conversationLane
		synctest.Wait()
		tester.Frame()
		if cv.Session.Loading || len(cv.Turns) != 1 || cv.Turns[0].User != second.String() || len(cv.Turns[0].Reply.Blocks) != 1 || cv.Turns[0].Reply.Blocks[0].Text != strings.Repeat("x", 128*1024) {
			t.Fatal("new mount did not receive its complete prepared transcript")
		}
		row := &cv.Turns[0]
		tester.Frame()
		if row != &cv.Turns[0] {
			t.Fatal("unchanged frame rebuilt prepared rows")
		}
		selected = protocol.SessionID{}
		tester.Frame()
		synctest.Wait()
		if len(r.conversationFeeds) != 0 || len(r.conversationCache) != 0 || len(r.conversationPresentations) != 0 || cv.Turns != nil {
			t.Fatal("an unmounted history remained resident")
		}
		if _, ok := controller.Conversation(second); ok {
			t.Fatal("unmounted feed kept its Client watch")
		}
	})
}

// The fixture distinguishes producing a large cold transcript from handing it
// to the UI. It excludes layout/paint; the transcript benchmarks cover those.
func BenchmarkConversationSwitchProjection(b *testing.B) {
	id := protocol.SessionID{1}
	state := client.ConversationState{Connected: true, Snapshot: fragmentedSnapshot(protocol.NodeID{1}, id, 2048)}
	r := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	b.Run("prepare", func(b *testing.B) {
		b.ReportAllocs()
		for b.Loop() {
			r.prepareConversation(id, state, "en")
		}
	})
	b.Run("apply", func(b *testing.B) {
		feed := &conversationFeed{changed: make(chan struct{}, 1)}
		r.conversationFeeds = map[protocol.SessionID]*conversationFeed{id: feed}
		r.conversationCache = map[protocol.SessionID]conversationCopy{}
		cv := &viewmodel.Conversation{ID: id.String()}
		// These completed text rows have no mutable detail or clock. Reusing the
		// fixture keeps producer setup out of both measurement and wall time.
		p := r.prepareConversation(id, state, "en")
		if len(p.view.Turns) != 1 || p.view.Turns[0].Reply.Blocks[0].Kind != conversation.TextBlock {
			b.Fatal("invalid fixture")
		}
		b.ReportAllocs()
		for b.Loop() {
			feed.latest.Store(p)
			r.takeConversationFeed(id, feed)
			r.projectConversation(cv, id, state, time.Unix(20, 0), "en")
		}
	})
}
