package shell

import (
	"cmp"
	"fmt"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

const workspaceBenchmarkTurns = 1000

type workspaceBenchmark struct {
	app    *App
	tester *ui.Tester
	target *ui.ListState
	x, y   float32
}

// newWorkspaceBenchmark uses the live presentation without backend hooks. Every
// conversation is completed text; no simulation, images, tools or models run.
// The five-pane topology matches three panes above two at a fixed window size.
// Reduced transparency avoids measuring headless software frost rasterization.
func newWorkspaceBenchmark(tb testing.TB, count int) *workspaceBenchmark {
	tb.Helper()
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
	for pane := range count {
		id := fmt.Sprintf("workspace-benchmark-%d", pane)
		turns := make([]conversation.Turn, workspaceBenchmarkTurns)
		for index := range turns {
			turns[index] = conversation.Turn{
				ID:   fmt.Sprintf("%s/turn-%d", id, index),
				User: fmt.Sprintf("Explain the result of task %d in this workspace.", index),
				Reply: conversation.Reply{Status: conversation.Completed, Elapsed: time.Second,
					Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: strings.Repeat(
						"The completed task keeps its result in the conversation. Each workspace pane retains its own scroll position and input. ", 4)}},
				},
			}
		}
		data.Unassigned = append(data.Unassigned, &viewmodel.Conversation{
			ID: id, Title: fmt.Sprintf("Workspace conversation %d", pane+1),
			Created: pane, Turns: turns, Session: conversation.Session{Connection: conversation.Connected},
		})
	}
	app := NewLive(data)
	app.NavOpen, app.PanelOpen, app.Effects, app.OverviewHidden = false, false, false, true
	app.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
		return &composer.Overview{Session: true}
	}
	leaf := func(index int) *panes.Node { return &panes.Node{Target: session(data.Unassigned[index].ID)} }
	if count == 5 {
		app.panes.Groups = []*panes.Node{{Axis: panes.Column, Sizes: []float32{0.5, 0.5}, Children: []*panes.Node{
			{Axis: panes.Row, Sizes: []float32{1.0 / 3, 1.0 / 3, 1.0 / 3}, Children: []*panes.Node{leaf(0), leaf(1), leaf(2)}},
			{Axis: panes.Row, Sizes: []float32{0.5, 0.5}, Children: []*panes.Node{leaf(3), leaf(4)}},
		}}}
	} else if count != 1 {
		tb.Fatalf("unsupported benchmark pane count %d", count)
	}
	app.focusPane(session(data.Unassigned[0].ID))
	tester := ui.NewTesterWith(app.View, 1800, 1000, ui.TesterOptions{
		Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true},
	})
	tester.SetLocale("en")
	for _, cv := range data.Unassigned {
		app.Messages.List(cv.ID).ScrollTo(workspaceBenchmarkTurns/2, ui.Center)
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
		tb.Fatalf("rendered %d transcripts, want %d", len(logs), count)
	}
	slices.SortFunc(logs, func(a, b ui.Rect) int {
		if order := cmp.Compare(a.Y, b.Y); order != 0 {
			return order
		}
		return cmp.Compare(a.X, b.X)
	})
	f := &workspaceBenchmark{app: app, tester: tester, target: app.Messages.List(data.Unassigned[0].ID),
		x: logs[0].X + logs[0].W/2, y: logs[0].Y + min(120, logs[0].H/3)}
	idle := make([][2]int, count)
	for pane, cv := range data.Unassigned {
		first, last := app.Messages.List(cv.ID).Visible()
		if first > workspaceBenchmarkTurns/2 || last < workspaceBenchmarkTurns/2 {
			tb.Fatalf("pane %d opened at turns %d..%d", pane, first, last)
		}
		idle[pane] = [2]int{first, last}
	}
	// Start away from the end with the jump button and pointer hover settled.
	tester.Scroll(f.x, f.y, 0, -120)
	tester.Advance(time.Second)
	tester.Frame()
	for pane := 1; pane < count; pane++ {
		first, last := app.Messages.List(data.Unassigned[pane].ID).Visible()
		if got := [2]int{first, last}; got != idle[pane] {
			tb.Fatalf("scrolling pane 0 moved idle pane %d from %v to %v", pane, idle[pane], got)
		}
	}
	return f
}

// BenchmarkWorkspaceScroll measures scrolling only the first conversation while
// the other panes remain idle. Headless CPU drawing is included; this fixture
// has no bitmap content and does not represent native GPU rendering cost.
func BenchmarkWorkspaceScroll(b *testing.B) {
	for _, count := range []int{1, 5} {
		b.Run(fmt.Sprintf("panes=%d", count), func(b *testing.B) {
			f := newWorkspaceBenchmark(b, count)
			b.ReportAllocs()
			b.ResetTimer()
			for b.Loop() {
				if first, _ := f.target.Visible(); first < 2 {
					b.StopTimer()
					f.target.ScrollTo(3*workspaceBenchmarkTurns/4, ui.Center)
					f.tester.Frame()
					f.tester.Advance(time.Second)
					b.StartTimer()
				}
				f.tester.Scroll(f.x, f.y, 0, -120)
			}
		})
	}
}

// BenchmarkWorkspaceSteady forces an otherwise unchanged frame, distinguishing
// the shell's baseline per-pane build cost from rows entering during scrolling.
func BenchmarkWorkspaceSteady(b *testing.B) {
	for _, count := range []int{1, 5} {
		b.Run(fmt.Sprintf("panes=%d", count), func(b *testing.B) {
			f := newWorkspaceBenchmark(b, count)
			b.ReportAllocs()
			b.ResetTimer()
			for b.Loop() {
				f.tester.Frame()
			}
		})
	}
}
