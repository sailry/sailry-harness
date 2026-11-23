package conversation

import (
	"math"
	"reflect"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// sweeps renders turns and reports which of texts the sweep lights: pixels in
// a text's box that differ from the still frame Reduce Motion draws, over a
// whole pass of the band. open names disclosure rows to click first.
func sweeps(t *testing.T, turns []Turn, open []string, texts ...string) map[string]bool {
	t.Helper()
	s := New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { View(c, s, "session", turns, nil, still{}) })
	}, 900, 800)
	tt.SetLocale("en")
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	for _, label := range open {
		if err := tt.Click(label); err != nil {
			t.Fatalf("no %q to open in %q: %v", label, tt.Texts(), err)
		}
		tt.Frame()
	}
	tt.Move(899, 799)
	tt.Frame()
	tt.Frame()
	rects := map[string]ui.Rect{}
	for _, text := range texts {
		r, ok := tt.Find(text)
		if !ok {
			t.Fatalf("no %q in %q", text, tt.Texts())
		}
		rects[text] = r
	}
	base := tt.Image()
	pix := append([]byte(nil), base.Pix...)
	stride := base.Stride
	tt.SetPreferences(ui.Preferences{})
	tt.Frame()
	lit := map[string]bool{}
	for range 50 {
		tt.Advance(50 * time.Millisecond)
		tt.Frame()
		now := tt.Image().Pix
		for text, r := range rects {
			for y := int(r.Y); y < int(r.Y+r.H) && !lit[text]; y++ {
				for x := int(r.X); x < int(r.X+r.W); x++ {
					o := y*stride + x*4
					if o+3 < len(now) && (now[o] != pix[o] || now[o+1] != pix[o+1] || now[o+2] != pix[o+2]) {
						lit[text] = true
						break
					}
				}
			}
		}
	}
	return lit
}

func runningCall(id, name, detail string, standalone bool) Call {
	return Call{ID: id, Name: name, Label: name, Icon: "file-text", Detail: detail, Code: true, Running: true, Standalone: standalone}
}

// settledCall is a call that returned; awaiting marks it as one of the batch
// the model's next output is awaited for.
func settledCall(id, name, detail string, standalone, awaiting bool) Call {
	call := runningCall(id, name, detail, standalone)
	call.Running, call.AwaitingModel = false, awaiting
	return call
}

func sweepTurn(phase Phase, blocks ...Block) []Turn {
	return []Turn{{ID: "turn", User: "Inspect", Reply: Reply{Status: Running, Phase: phase, Blocks: blocks}}}
}

func callBlocks(calls ...Call) []Block {
	var blocks []Block
	for _, call := range calls {
		blocks = append(blocks, Block{Kind: ToolsBlock, Calls: []Call{call}})
	}
	return blocks
}

func expectSweeps(t *testing.T, lit map[string]bool, want map[string]bool) {
	t.Helper()
	for text, sweep := range want {
		if lit[text] != sweep {
			t.Errorf("%q swept %v, want %v (lit %v)", text, lit[text], sweep, lit)
		}
	}
}

// Every running call sweeps its own row: one alone, calls running together
// in blocks of their own or in one block of grouped calls, beside the task's progress,
// whose row keeps its own sweep. Subagent rows are not involved.
func TestRunningToolRowsSweep(t *testing.T) {
	t.Run("single", func(t *testing.T) {
		lit := sweeps(t, sweepTurn(ExecutingTool, Block{Kind: ToolsBlock, Calls: []Call{runningCall("a", "read", "a.go", false)}}), nil, "a.go")
		expectSweeps(t, lit, map[string]bool{"a.go": true})
	})
	t.Run("concurrent rows", func(t *testing.T) {
		blocks := callBlocks(runningCall("a", "read", "a.go", true), runningCall("b", "grep", "b.go", true), runningCall("c", "list", "c.go", true))
		expectSweeps(t, sweeps(t, sweepTurn(ExecutingTool, blocks...), nil, "a.go", "b.go", "c.go"), map[string]bool{"a.go": true, "b.go": true, "c.go": true})
	})
	t.Run("concurrent block", func(t *testing.T) {
		// One block of calls of one name and of another folds into one
		// group whose closed header, the latest call, sweeps; opened,
		// consecutive same-tool calls form a sweeping branch whose leaves
		// keep their own sweep state.
		calls := []Call{runningCall("a", "read", "a.go", false), runningCall("b", "read", "b.go", false), runningCall("c", "read", "c.go", false), runningCall("d", "grep", "d.go", false)}
		calls[1].Running = false
		turns := sweepTurn(ExecutingTool, Block{Kind: ToolsBlock, Calls: calls})
		group := "d.go"
		expectSweeps(t, sweeps(t, turns, nil, group), map[string]bool{group: true})
		expectSweeps(t, sweeps(t, turns, []string{group}, "read", "d.go"), map[string]bool{"read": true, "d.go": true})
		lit := sweeps(t, turns, []string{group, "read"}, "a.go", "b.go", "c.go", "d.go")
		expectSweeps(t, lit, map[string]bool{"a.go": true, "b.go": false, "c.go": true, "d.go": true})
	})
	t.Run("with progress", func(t *testing.T) {
		progress := Block{Kind: ProgressBlock, Steps: []Step{{Text: "Inspect", State: StepInProgress}, {Text: "Report"}}}
		tools := callBlocks(runningCall("a", "read", "a.go", true), runningCall("b", "grep", "b.go", true))
		head := i18n.In("en", "task_progress")
		lit := sweeps(t, sweepTurn(ExecutingTool, append([]Block{progress}, tools...)...), nil, head, "a.go", "b.go")
		expectSweeps(t, lit, map[string]bool{head: true, "a.go": true, "b.go": true})
	})
}

func TestRunningCommandSweepPreservesEllipsis(t *testing.T) {
	for _, tc := range []struct {
		locale, label, detail string
	}{
		{"en", "Command", `go test ./internal/desktop/kit ./internal/desktop/live`},
		{"zh-CN", "命令", `printf '%s\n' '测试运行中的命令行扫光和省略号，保留可见文字的完整字形'`},
	} {
		t.Run(tc.locale, func(t *testing.T) {
			call := Call{ID: "command", Name: "run_command", Label: tc.label, Icon: "square-terminal",
				Detail: tc.detail, Code: true, Running: true, Terminal: true, Output: &Output{Text: "working", Command: true}}
			state := New()
			var naturalWidth float32
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				naturalWidth, _ = c.MeasureText(0, ui.Span{Text: tc.detail, Font: "monospace", Size: textSize(c) - 1})
				ui.Column(c).Fill().Padding(8).Background(theme.Of(c).Background).Children(func() { state.row(c, call) })
			}, 760, 40)
			tester.SetLocale(tc.locale)
			for _, width := range []int{760, 200, 241, 760} {
				tester.SetSize(width, 40)
				tester.SetPreferences(ui.Preferences{ReduceMotion: true})
				r, ok := tester.Find(tc.detail)
				if !ok || r.W < 40 || r.X+r.W > float32(width) {
					t.Fatalf("width %d: command detail has unexpected bounds %+v (found %v)", width, r, ok)
				}
				if (r.W+1 < naturalWidth) != (width < 760) {
					t.Fatalf("width %d: command box %v, natural text width %v", width, r.W, naturalWidth)
				}
				still := tester.Image()
				background := still.RGBAAt(0, 0)
				left, right := int(math.Ceil(float64(r.X))), int(math.Floor(float64(r.X+r.W)))
				top, bottom := int(math.Ceil(float64(r.Y))), int(math.Floor(float64(r.Y+r.H)))
				lastInk := left
				for y := top; y < bottom; y++ {
					for x := left; x < right; x++ {
						if still.RGBAAt(x, y) != background {
							lastInk = max(lastInk, x)
						}
					}
				}
				if lastInk <= left {
					t.Fatalf("width %d: no visible command text", width)
				}
				tester.SetPreferences(ui.Preferences{})
				litPrefix, litEnd, spill := false, false, 0
				for range 20 {
					tester.Advance(100 * time.Millisecond)
					frame := tester.Image()
					for y := top; y < bottom; y++ {
						for x := left; x < right; x++ {
							base := still.RGBAAt(x, y)
							if base == frame.RGBAAt(x, y) {
								continue
							}
							if base == background {
								spill++
							} else {
								litPrefix = litPrefix || x < (left+lastInk)/2
								litEnd = litEnd || x == lastInk
							}
						}
					}
				}
				if spill != 0 || !litPrefix || !litEnd {
					t.Errorf("width %d: prefix lit %t, end lit %t, %d changes on background pixels", width, litPrefix, litEnd, spill)
				}
			}
		})
	}
}

// While the model's next output is awaited, every call of the returned batch
// (Call.AwaitingModel) sweeps its row, and the latest one's sweep stands in
// for the waiting phase; an earlier
// batch, and a batch call that failed, awaits approval or was denied, stay
// still, and the task's progress keeps its own sweep.
func TestAwaitingModelRowsSweep(t *testing.T) {
	earlier := []Call{settledCall("a", "read", "a.go", true, false), settledCall("b", "grep", "b.go", true, false)}
	batch := []Call{settledCall("c", "read", "c.go", true, true), settledCall("d", "grep", "d.go", true, true), settledCall("e", "list", "e.go", true, true)}
	t.Run("batch rows", func(t *testing.T) {
		lit := sweeps(t, sweepTurn(AwaitingResponse, callBlocks(append(earlier, batch...)...)...), nil, "a.go", "b.go", "c.go", "d.go", "e.go")
		expectSweeps(t, lit, map[string]bool{"a.go": false, "b.go": false, "c.go": true, "d.go": true, "e.go": true})
	})
	t.Run("batch block", func(t *testing.T) {
		calls := []Call{settledCall("a", "read", "a.go", false, false), settledCall("b", "read", "b.go", false, true), settledCall("c", "grep", "c.go", false, true)}
		turns := sweepTurn(AwaitingResponse, Block{Kind: ToolsBlock, Calls: calls})
		group := "c.go"
		expectSweeps(t, sweeps(t, turns, nil, group), map[string]bool{group: true})
		expectSweeps(t, sweeps(t, turns, []string{group}, "read", "c.go"), map[string]bool{"read": true, "c.go": true})
		lit := sweeps(t, turns, []string{group, "read"}, "a.go", "b.go", "c.go")
		expectSweeps(t, lit, map[string]bool{"a.go": false, "b.go": true, "c.go": true})
	})
	t.Run("with progress", func(t *testing.T) {
		head := i18n.In("en", "task_progress")
		progress := Block{Kind: ProgressBlock, Steps: []Step{{Text: "Inspect", State: StepCompleted}, {Text: "Report", State: StepInProgress}}}
		lit := sweeps(t, sweepTurn(AwaitingResponse, append(callBlocks(batch...), progress)...), nil, head, "c.go", "e.go")
		expectSweeps(t, lit, map[string]bool{head: true, "c.go": true, "e.go": true})
	})
	t.Run("guarded rows", func(t *testing.T) {
		failed, pending, denied := batch[0], batch[1], batch[2]
		failed.Failed = true
		pending.Pending, pending.State = true, Pending
		denied.Warning, denied.State = true, Denied
		lit := sweeps(t, sweepTurn(AwaitingResponse, callBlocks(failed, pending, denied)...), nil, "c.go", "d.go", "e.go")
		expectSweeps(t, lit, map[string]bool{"c.go": false, "d.go": false, "e.go": false})
	})
	t.Run("offline cached rows", func(t *testing.T) {
		running := runningCall("running", "read", "running.go", true)
		turns := sweepTurn(AwaitingResponse, callBlocks(running, batch[0])...)
		turns[0].Reply.Status = Unsynced
		expectSweeps(t, sweeps(t, turns, nil, "running.go", "c.go"), map[string]bool{"running.go": false, "c.go": false})
	})
}

// A block's calls keep their own guards: a background job, an approval
// and a denial stay still among calls that sweep while the model is awaited.
// The closed group's header is the latest call, a denial, and stays still.
func TestAwaitingModelBlockGuards(t *testing.T) {
	background := settledCall("bg", "read", "bg.go", false, true)
	background.Background = &Background{Outcome: OutcomeCompleted}
	pending := settledCall("p", "read", "p.go", false, true)
	pending.Pending, pending.State = true, Pending
	denied := settledCall("d", "grep", "d.go", false, true)
	denied.Warning, denied.State = true, Denied
	calls := []Call{settledCall("a", "read", "a.go", false, true), background, pending, denied}
	turns := sweepTurn(AwaitingResponse, Block{Kind: ToolsBlock, Calls: calls})
	group := "d.go"
	expectSweeps(t, sweeps(t, turns, nil, group), map[string]bool{group: false})
	lit := sweeps(t, turns, []string{group}, "a.go", "bg.go", "p.go", "d.go")
	expectSweeps(t, lit, map[string]bool{"a.go": true, "bg.go": false, "p.go": false, "d.go": false})
}

// A group's header is its latest call, label and state alike.
func TestToolGroupHeadState(t *testing.T) {
	read := Call{ID: "r", Name: "read", Label: "Read file"}
	for _, last := range []Call{
		{ID: "s", Label: "Search", Running: true},
		{ID: "c", Label: "Command", Pending: true, State: Pending},
		{ID: "f", Label: "Command", Failed: true},
		{ID: "w", Label: "Edit", Warning: true, State: Denied, wrap: true},
	} {
		head := groupHead([]Call{read, {Label: "Other", Failed: true}, last})
		want := last
		want.wrap = false
		if !reflect.DeepEqual(head, want) {
			t.Fatalf("header %+v, want %+v", head, want)
		}
	}
}

// After the reply's first output the waiting phase no longer shows, whether
// the latest call is awaited or failed.
func TestAwaitingPhaseBehindSweep(t *testing.T) {
	waiting := i18n.In("en", "turn_awaiting_response")
	awaited := settledCall("a", "read", "a.go", true, true)
	failed := settledCall("b", "grep", "b.go", true, true)
	failed.Failed = true
	progress := Block{Kind: ProgressBlock, Steps: []Step{{Text: "Inspect", State: StepInProgress}}}
	for _, row := range []struct {
		name   string
		blocks []Block
		shown  bool
	}{
		{"awaited", callBlocks(awaited), false},
		{"awaited above progress", append(callBlocks(awaited), progress), false},
		{"failed", callBlocks(awaited, failed), false},
	} {
		t.Run(row.name, func(t *testing.T) {
			turns := sweepTurn(AwaitingResponse, row.blocks...)
			s := New()
			tt := ui.NewTester(func(c *ui.Context) { View(c, s, "session", turns, nil, still{}) }, 900, 600)
			tt.SetLocale("en")
			tt.SetPreferences(ui.Preferences{ReduceMotion: true})
			tt.Frame()
			if tt.HasText(waiting) != row.shown {
				t.Fatalf("waiting phase shown %v, want %v: %q", !row.shown, row.shown, tt.Texts())
			}
		})
	}
}
