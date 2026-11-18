package live

import (
	"cmp"
	"context"
	"fmt"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const liveWorkspaceBenchmarkTurns = 1000

type liveWorkspaceBenchmark struct {
	runtime *Runtime
	tester  *ui.Tester
	target  *ui.ListState
	x, y    float32
}

type liveWorkspaceBenchmarkTransport struct {
	conversationTestTransport
	conversations map[protocol.SessionID]protocol.ConversationSnapshot
}

func (transport *liveWorkspaceBenchmarkTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "conversation" {
		return transport.conversationTestTransport.Subscribe(ctx, topic)
	}
	id, valid := detail[protocol.SessionID](topic.Data)
	snapshot, exists := transport.conversations[id]
	if !valid || !exists {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "unknown benchmark conversation"}
	}
	subscription := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	subscription.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
	return subscription, nil
}

// newLiveWorkspaceBenchmark exercises Runtime.conversation over real Client
// observations of immutable fixture snapshots. It includes source-copy caching,
// live projection, enrichment and controls, without Node or model execution.
func newLiveWorkspaceBenchmark(b *testing.B, count int) *liveWorkspaceBenchmark {
	b.Helper()
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider, "fixture"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Benchmark workspace", Path: "/fixture"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 1, Enabled: true,
		Models: []protocol.ConversationModel{{ID: "fixture"}}}}
	conversations := map[protocol.SessionID]protocol.ConversationSnapshot{}
	ids := make([]protocol.SessionID, count)
	for pane := range count {
		id := protocol.SessionID{byte(pane + 1)}
		ids[pane] = id
		snapshot.Sessions = append(snapshot.Sessions, protocol.Session{ID: id, Revision: 1,
			Activity: protocol.ActivitySummary{Title: fmt.Sprintf("Workspace conversation %d", pane+1)}, Project: &project, Worktree: worktree,
			Config: config, Roles: protocol.DefaultRoleSnapshot()})
		page := protocol.DefaultConversationPage()
		page.Session, page.Revision = id, 1
		for index := range liveWorkspaceBenchmarkTurns {
			number := index + 1
			turn := protocol.TurnID{byte(number), byte(number >> 8)}
			started, finished := int64(1_700_000_000_000+index*2000), int64(1_700_000_001_000+index*2000)
			page.Runs = append(page.Runs, protocol.ConversationRun{Session: id, Turn: turn, Worktree: worktree,
				Sequence: uint64(number), Revision: 1, Kind: protocol.ConversationRunKindTask,
				Status: protocol.ConversationStatusCompleted, StartedMs: &started, FinishedMs: &finished})
			page.Entries = append(page.Entries,
				protocol.ConversationEntry{ID: fmt.Sprintf("%s/user-%d", id, index), Turn: turn, Sequence: uint64(2*index + 1), Author: "user",
					Parts: []protocol.ConversationPart{{Kind: "text", Data: fmt.Sprintf("Explain the result of task %d in this workspace.", index)}}},
				protocol.ConversationEntry{ID: fmt.Sprintf("%s/reply-%d", id, index), Turn: turn, Sequence: uint64(2*index + 2), Author: "assistant",
					Parts: []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat(
						"The completed task keeps its result in the conversation. Each workspace pane retains its own scroll position and input. ", 4)}}})
		}
		accepted := protocol.DefaultConversationSnapshot()
		accepted.Node, accepted.Sequence, accepted.Page = node, 1, page
		conversations[id] = accepted
	}
	transport := &liveWorkspaceBenchmarkTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot}, conversations: conversations,
	}
	controller := client.New(transport)
	if err := controller.Start(b.Context()); err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { _ = controller.Close() })
	runtime, err := NewWithPreferences(b.Context(), controller, preferences.Open(filepath.Join(b.TempDir(), "preferences.json")))
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { _ = runtime.Close() })
	changed := make(chan struct{}, count)
	for _, id := range ids {
		release, err := controller.WatchConversation(b.Context(), id, func() {
			runtime.conversationChanged(id)
			select {
			case changed <- struct{}{}:
			default:
			}
		})
		if err != nil {
			b.Fatal(err)
		}
		runtime.conversationWatches[id] = release
	}
	deadline := time.NewTimer(10 * time.Second)
	defer deadline.Stop()
	for {
		ready := true
		for _, id := range ids {
			state, exists := controller.Conversation(id)
			if !exists || !state.Connected || state.Loading || state.Err != nil || len(state.Snapshot.Page.Runs) != liveWorkspaceBenchmarkTurns {
				ready = false
				if state.Err != nil {
					b.Fatalf("benchmark conversation %s: %v", id, state.Err)
				}
				break
			}
		}
		if ready {
			break
		}
		select {
		case <-changed:
		case <-deadline.C:
			b.Fatal("benchmark conversations did not become ready")
		case <-b.Context().Done():
			b.Fatal(b.Context().Err())
		}
	}
	app := runtime.App
	app.Sidebar.Selected = ids[0].String()
	app.Effects = false
	prefs := app.Prefs()
	prefs.NavOpen, prefs.PanelOpen, prefs.OverviewHidden = false, false, true
	leaf := func(index int) *panes.Node {
		return &panes.Node{Target: panes.Target{Kind: panes.Session, ID: ids[index].String()}}
	}
	if count == 5 {
		prefs.Panes = []*panes.Node{{Axis: panes.Column, Sizes: []float32{0.5, 0.5}, Children: []*panes.Node{
			{Axis: panes.Row, Sizes: []float32{1.0 / 3, 1.0 / 3, 1.0 / 3}, Children: []*panes.Node{leaf(0), leaf(1), leaf(2)}},
			{Axis: panes.Row, Sizes: []float32{0.5, 0.5}, Children: []*panes.Node{leaf(3), leaf(4)}},
		}}}
	} else if count != 1 {
		b.Fatalf("unsupported benchmark pane count %d", count)
	}
	app.SetPrefs(prefs)
	app.Hooks.BeforeFrame = func(*ui.Context) {
		runtime.beginConversationFrame()
	}
	app.Hooks.AfterFrame = runtime.finishConversationFrame
	tester := ui.NewTesterWith(app.View, 1800, 1000, ui.TesterOptions{
		Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true},
	})
	tester.SetLocale("en")
	for _, id := range ids {
		app.Messages.List(id.String()).ScrollTo(liveWorkspaceBenchmarkTurns/2, ui.Center)
	}
	tester.Advance(time.Second)
	tester.Frame()
	tester.Frame()
	var logs []ui.Rect
	for _, entry := range tester.AccessAll() {
		if entry.Role == ui.RoleLog && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
			logs = append(logs, entry.Bounds)
		}
	}
	if len(logs) != count {
		b.Fatalf("rendered %d transcripts, want %d", len(logs), count)
	}
	slices.SortFunc(logs, func(a, b ui.Rect) int {
		if order := cmp.Compare(a.Y, b.Y); order != 0 {
			return order
		}
		return cmp.Compare(a.X, b.X)
	})
	f := &liveWorkspaceBenchmark{runtime: runtime, tester: tester, target: app.Messages.List(ids[0].String()),
		x: logs[0].X + logs[0].W/2, y: logs[0].Y + min(120, logs[0].H/3)}
	tester.Scroll(f.x, f.y, 0, -120)
	tester.Advance(time.Second)
	tester.Frame()
	for _, id := range ids {
		if cv := app.Data.Find(id.String()); cv == nil || len(cv.Turns) != liveWorkspaceBenchmarkTurns || cv.Running() || cv.Playing() {
			b.Fatalf("conversation %s is not a completed %d-turn fixture", id, liveWorkspaceBenchmarkTurns)
		}
	}
	return f
}

func BenchmarkLiveWorkspaceScroll(b *testing.B) {
	for _, count := range []int{1, 5} {
		b.Run(fmt.Sprintf("panes=%d", count), func(b *testing.B) {
			f := newLiveWorkspaceBenchmark(b, count)
			b.ReportAllocs()
			b.ResetTimer()
			for b.Loop() {
				if first, _ := f.target.Visible(); first < 2 {
					b.StopTimer()
					f.target.ScrollTo(3*liveWorkspaceBenchmarkTurns/4, ui.Center)
					f.tester.Frame()
					f.tester.Advance(time.Second)
					b.StartTimer()
				}
				f.tester.Scroll(f.x, f.y, 0, -120)
			}
		})
	}
}

func BenchmarkLiveWorkspaceSteady(b *testing.B) {
	for _, count := range []int{1, 5} {
		b.Run(fmt.Sprintf("panes=%d", count), func(b *testing.B) {
			f := newLiveWorkspaceBenchmark(b, count)
			b.ReportAllocs()
			b.ResetTimer()
			for b.Loop() {
				f.tester.Frame()
			}
		})
	}
}
