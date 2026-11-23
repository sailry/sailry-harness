package conversation

import (
	"cmp"
	"context"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// flatRowsShot writes the frame to $SAILRY_SNAPSHOTS/conversation-tool-rows-<name>.png, when set.
func flatRowsShot(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, "conversation-tool-rows-"+name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
}

// rowButtons are the buttons whose names start with label, top to bottom.
func rowButtons(tt *ui.Tester, label string) []ui.AccessInfo {
	var out []ui.AccessInfo
	for _, a := range tt.AccessAll() {
		if a.Role == ui.RoleButton && !a.Offscreen && strings.HasPrefix(a.Name, label) {
			out = append(out, a)
		}
	}
	slices.SortFunc(out, func(a, b ui.AccessInfo) int { return cmp.Compare(a.Bounds.Y, b.Bounds.Y) })
	return out
}

// clickRow clicks the lowest button named label: the row of an open group
// rather than the group's header, which repeats its latest call.
func clickRow(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	rows := rowButtons(tt, label)
	if len(rows) == 0 {
		t.Fatalf("no row %q: %q", label, tt.Texts())
	}
	r := rows[len(rows)-1].Bounds
	tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
}

func textCount(tt *ui.Tester, text string) int {
	n := 0
	for _, s := range tt.Texts() {
		if s == text {
			n++
		}
	}
	return n
}

// A group initially hides the individual headers, whatever their calls'
// states: a running call and a failure fold with the settled ones, and the
// header shows the run's latest call. Opening it reveals the original call
// order and each row still opens straight onto its own result.
func TestConsecutiveToolRowsFold(t *testing.T) {
	calls := []Call{
		{ID: "r1", Name: "read", Label: "Read file", Icon: "file-text", Detail: "a.go", Code: true, Output: &Output{Text: "out a"}},
		{ID: "r2", Name: "read", Label: "Read file", Icon: "file-text", Detail: "b.go", Code: true, Output: &Output{Text: "out b"}},
		{ID: "g1", Name: "grep", Label: "Search", Icon: "search", Detail: "c.go", Code: true, Output: &Output{Text: "out c"}},
		runningCall("r4", "read", "current.go", false),
		{ID: "err", Name: "command", Label: "Command", Code: true, Failed: true, Error: "Command failed"},
	}
	details := []string{"a.go", "b.go", "c.go", "current.go"}
	const label = "Command"
	for _, size := range []uint8{12, 14, 18} {
		for _, width := range []int{480, 900} {
			t.Run(fmt.Sprintf("size_%d_width_%d", size, width), func(t *testing.T) {
				s := New()
				running := slices.Clone(calls)
				loads := 0
				running[1].Load = func(apply func(*Output)) func() {
					loads++
					apply(&Output{Text: "out b"})
					return nil
				}
				turns := []Turn{{ID: "turn", User: "Inspect", Reply: Reply{Status: Running, Phase: ExecutingTool, Blocks: callBlocks(running...)}}}
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { View(c, s, "session", turns, nil, still{}) })
				}, width, 700)
				tt.SetLocale("en")
				tt.SetPreferences(ui.Preferences{ReduceMotion: true})
				tt.Frame()
				groups := rowButtons(tt, label)
				if len(groups) != 1 || groups[0].Expanded || textCount(tt, label) != 1 || tt.HasText("Command failed") {
					t.Fatalf("every call of the run, active and failed ones included, should fold into one group: %q", tt.Texts())
				}
				if tt.HasText("5") {
					t.Fatalf("the folded group shows its call count: %q", tt.Texts())
				}
				for _, detail := range details {
					if tt.HasText(detail) {
						t.Fatalf("the folded group displays %q: %q", detail, tt.Texts())
					}
				}
				flatRowsShot(t, tt, fmt.Sprintf("closed-%d-%d", size, width))
				if err := tt.Click(label); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
				if loads != 0 {
					t.Fatal("opening a group fetched a call's details")
				}
				var rows []ui.Rect
				for i, detail := range details {
					if n := textCount(tt, detail); n != 1 {
						t.Fatalf("%q shows %d times, want once: %q", detail, n, tt.Texts())
					}
					r, _ := tt.Find(detail)
					if i > 0 && r.Y <= rows[i-1].Y {
						t.Fatalf("%q at %v is not below the call before it at %v", detail, r, rows[i-1])
					}
					rows = append(rows, r)
				}
				// The failure is the run's last row, in its place after the
				// running call; the header above repeats it.
				texts := tt.Texts()
				at := slices.Index(texts, "current.go")
				if textCount(tt, label) != 2 || slices.Index(texts, label) > slices.Index(texts, "a.go") || at < 0 || !slices.Contains(texts[at:], label) {
					t.Fatalf("the failed call is not the last row of the open group: %q", tt.Texts())
				}
				// The calls with a result open; the running one has none yet.
				buttons := append(rowButtons(tt, "Read file"), rowButtons(tt, "Search")...)
				if len(buttons) != len(details)-1 {
					t.Fatalf("%d tool row buttons, want one per grouped call: %+v", len(buttons), buttons)
				}
				for _, b := range buttons {
					if b.Bounds.X != buttons[0].Bounds.X || b.Expanded {
						t.Fatalf("row %q at %v is not a closed row on the first one's level %v", b.Name, b.Bounds, buttons[0].Bounds)
					}
				}
				if err := tt.Click("b.go"); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
				if !tt.HasText("out b") || tt.HasText("out a") || tt.HasText("out c") || loads != 1 {
					t.Fatalf("one click did not open its own result alone: %q, %d detail loads", tt.Texts(), loads)
				}
				for _, detail := range details {
					if n := textCount(tt, detail); n != 1 {
						t.Fatalf("opened, %q shows %d times: %q", detail, n, tt.Texts())
					}
				}
				result, _ := tt.Find("out b")
				if next, _ := tt.Find("c.go"); result.Y <= rows[1].Y || result.Y+result.H > next.Y {
					t.Fatalf("the result %v is not between its row %v and the next %v", result, rows[1], next)
				}
				flatRowsShot(t, tt, fmt.Sprintf("open-%d-%d", size, width))
			})
		}
	}
}

func TestFailedToolGroupRequiresManualDisclosure(t *testing.T) {
	const failure = "the plugin could not read the changes\nCode: changes_unavailable"
	s := New()
	loads := 0
	calls := []Call{
		{ID: "review", Name: "review_scope", Label: "Review scope", Running: true},
		{ID: "read", Name: "read_file", Label: "Read file", Output: &Output{Text: "successful output"}},
	}
	turns := []Turn{{ID: "run", User: "Review", Reply: Reply{Status: Running, Phase: ExecutingTool,
		Blocks: []Block{{Kind: ToolsBlock, Calls: calls}}}}}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		View(c, s, "session", turns, nil, still{})
	}, 900, 700)
	tt.SetLocale("en")
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	if !tt.HasText("Read file") || tt.HasText("Review scope") {
		t.Fatalf("consecutive calls were not folded: %q", tt.Texts())
	}
	calls[0].Running, calls[0].Failed = false, true
	calls[0].Output = &Output{}
	calls[0].Load = func(apply func(*Output)) func() {
		loads++
		apply(&Output{Failure: failure})
		return nil
	}
	turns[0].Reply.Status, turns[0].Reply.Phase = Failed, PhaseNone
	tt.Frame()
	if !tt.HasText("Read file") || tt.HasText("Review scope") || tt.HasText(failure) || loads != 0 {
		t.Fatalf("the failed run did not keep the group folded: %q, %d detail loads", tt.Texts(), loads)
	}
	if err := tt.Click("Read file"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("Review scope") || tt.HasText(failure) || loads != 0 {
		t.Fatalf("opening the group did not leave the tool result folded: %q, %d detail loads", tt.Texts(), loads)
	}
	if err := tt.Click("Review scope"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText(failure) || tt.HasText("successful output") || loads != 1 {
		t.Fatalf("opening the failed tool did not reveal just its result: %q, %d detail loads", tt.Texts(), loads)
	}
}

// A group's header is its latest call, in that call's own state and colour,
// even while an earlier call of the run is still going or has failed.
func TestToolGroupHeadFollowsLatestCall(t *testing.T) {
	calls := []Call{
		{ID: "build", Name: "run_command", Label: "Run command", Running: true},
		{ID: "read", Name: "read_file", Label: "Read file", Output: &Output{Text: "done"}},
	}
	if head := groupHead(calls); head.ID != "read" || !head.Running || head.Failed {
		t.Fatalf("the header did not show the latest call sweeping for a running group: %+v", head)
	}
	calls[1].Running, calls[1].Output = true, nil
	if head := groupHead(calls); !head.Running {
		t.Fatalf("the header lost the latest call's running state: %+v", head)
	}
	calls[0].Running, calls[0].Failed = false, true
	calls[1].Running, calls[1].Output = false, &Output{Text: "done"}
	if head := groupHead(calls); head.Running || head.Failed {
		t.Fatalf("the header took an earlier call's state: %+v", head)
	}
}

// A row listing the files its call touched opens onto their links, once
// each, without the aggregate preview or a Copy action; a failure still
// shows.
func TestToolRowPathList(t *testing.T) {
	s := New()
	var links []string
	s.OpenLink = func(_ *ui.Context, _, link string) bool { links = append(links, link); return true }
	calls := []Call{
		{ID: "find", Name: "find", Label: "Find files", Icon: "search", Output: &Output{Text: "aggregate", Paths: []string{"a/one.go", "a/two.go", "a/one.go"}}},
		{ID: "find2", Name: "find", Label: "Find more", Icon: "search", Output: &Output{Failure: "partial listing", Paths: []string{"b/three.go", "b/four.go"}, Notices: []string{"One folder was skipped"}}},
	}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		s.tools(c, calls, false)
	}, 900, 600)
	tt.SetLocale("en")
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	// The two calls fold together, the failure included, under one header.
	if textCount(tt, "Find files") != 0 || tt.HasText("b/three.go") || textCount(tt, "Find more") != 1 {
		t.Fatalf("the calls did not fold into one group: %q", tt.Texts())
	}
	if err := tt.Click("Find more"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if tt.HasText("a/one.go") || tt.HasText("aggregate") || tt.HasText("b/three.go") || tt.HasText("partial listing") {
		t.Fatalf("opening the group exposed results from closed rows: %q", tt.Texts())
	}
	clickRow(t, tt, "Find more")
	tt.Frame()
	if !tt.HasText("b/three.go") || !tt.HasText("partial listing") || !tt.HasText("One folder was skipped") || tt.HasText("Partial results") {
		t.Fatalf("opening a failed path list lost its details: %q", tt.Texts())
	}
	if err := tt.Click("Find files"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	// A path row is a button labelled with its path.
	if len(rowButtons(tt, "a/one.go")) != 1 || len(rowButtons(tt, "a/two.go")) != 1 || tt.HasText("aggregate") {
		t.Fatalf("the path row: %q", tt.Texts())
	}
	if _, shown := tt.AccessByRole(ui.RoleButton, i18n.In("en", "tool_copy")); shown {
		t.Fatal("a path list offers a raw-result Copy action")
	}
	if err := tt.Click("a/two.go"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if len(links) != 1 || !strings.Contains(links[0], "a/two.go") {
		t.Fatalf("the path opened %q", links)
	}
	clickRow(t, tt, "Find more")
	tt.Frame()
	if tt.HasText("b/three.go") || tt.HasText("partial listing") || !tt.HasText("a/one.go") {
		t.Fatalf("closing a failed path list changed a neighboring result: %q", tt.Texts())
	}
}

// A command's row opens from the keyboard onto its last 100 lines and no
// note of the lines left out, Copy takes the whole original output, and the
// keyboard closes the row again.
func TestToolRowCommandKeyboard(t *testing.T) {
	var lines []string
	for i := range 130 {
		lines = append(lines, fmt.Sprintf("line %03d", i))
	}
	text := strings.Join(lines, "\n")
	s := New()
	calls := []Call{
		{ID: "cmd", Name: "command", Label: "Run", Icon: "square-terminal", Detail: "make", Code: true, Standalone: true, Output: &Output{Text: text, Command: true}},
		{ID: "cmd2", Name: "command", Label: "Run", Icon: "square-terminal", Detail: "make test", Code: true, Standalone: true, Output: &Output{Text: "ok", Command: true}},
	}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		s.tools(c, calls, true)
	}, 900, 600)
	tt.SetLocale("en")
	tt.Frame()
	before := tt.Texts()
	tt.Key(0, ui.KeyTab)
	tt.Frame()
	rows := rowButtons(tt, "Run")
	if len(rows) != 2 || !rows[0].Focused || rows[1].Focused {
		t.Fatalf("Tab did not focus the first row: %+v", rows)
	}
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	shown, hidden := lastLines(text)
	if hidden != 30 {
		t.Fatalf("hidden %d lines, want 30", hidden)
	}
	copyLabel := i18n.In("en", "tool_copy")
	var added []string
	for _, s := range tt.Texts() {
		if !slices.Contains(before, s) && s != copyLabel {
			added = append(added, s)
		}
	}
	if len(added) != 1 || added[0] != shown {
		t.Fatalf("Enter opened %q, want the last 100 lines alone", added)
	}
	if rows := rowButtons(tt, "Run"); !rows[0].Expanded || rows[1].Expanded {
		t.Fatalf("the open row is not the focused one: %+v", rows)
	}
	b, ok := tt.AccessByRole(ui.RoleButton, copyLabel)
	if !ok {
		t.Fatal("no Copy action on the command's output")
	}
	tt.Move(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	if err := tt.Click(copyLabel); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if tt.Clipboard() != text {
		t.Fatalf("Copy took %d bytes, want the original %d", len(tt.Clipboard()), len(text))
	}
	focus := func() {
		t.Helper()
		for range 8 {
			if rows := rowButtons(tt, "Run"); rows[0].Focused {
				return
			}
			tt.Key(ui.Shift, ui.KeyTab)
			tt.Frame()
		}
		t.Fatalf("Shift-Tab did not return to the row: %+v", rowButtons(tt, "Run"))
	}
	focus()
	tt.Key(0, ui.KeySpace)
	tt.Frame()
	if tt.HasText(shown) {
		t.Fatalf("Space did not close the row: %q", tt.Texts())
	}
}

func TestPendingToolCopyYieldsToLaterCopy(t *testing.T) {
	s := New()
	var pending context.Context
	output := Output{Text: "Retained preview", Command: true, Copy: func(ctx context.Context) { pending = ctx }}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Children(func() {
			s.output(c, &output, false)
			ui.Button(c, "Copy newer text").OnClick(func() { s.copyText(c, "newer text") })
		})
	}, 900, 600)
	tt.SetLocale("en")
	tt.Frame()
	copyLabel := i18n.In("en", "tool_copy")
	button, ok := tt.AccessByRole(ui.RoleButton, copyLabel)
	if !ok {
		t.Fatal("retained output is missing its Copy action")
	}
	tt.Move(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	tt.Frame()
	if err := tt.Click(copyLabel); err != nil || pending == nil || pending.Err() != nil {
		t.Fatalf("asynchronous copy did not start: %v", err)
	}
	if err := tt.Click("Copy newer text"); err != nil {
		t.Fatal(err)
	}
	if pending.Err() == nil || tt.Clipboard() != "newer text" {
		t.Fatal("a newer copy left an older request able to overwrite the clipboard")
	}
}

// A settled turn's rows stay still, whatever their calls last reported.
func TestHistoryToolRowsStill(t *testing.T) {
	calls := []Call{runningCall("a", "read", "a.go", false), settledCall("b", "grep", "b.go", false, true)}
	turns := []Turn{{ID: "turn", User: "Inspect", Reply: Reply{Status: Completed, Blocks: []Block{{Kind: ToolsBlock, Calls: calls}, {Kind: TextBlock, Text: "Done"}}}}}
	lit := sweeps(t, turns, []string{i18n.In("en", "turn_completed"), "b.go"}, "a.go", "b.go")
	expectSweeps(t, lit, map[string]bool{"a.go": false, "b.go": false})
}

// While a run streams, its calls start, settle and fail in one closed group
// whose header, showing the latest call, stays where it first appeared; the
// task's progress card keeps its place below it instead of splitting the run.
func TestStreamingToolRunKeepsItsPlace(t *testing.T) {
	read := func(id, path string) Call {
		return Call{ID: id, Name: "read_file", Label: "Read file", Icon: "file-text", Detail: path, Code: true, Output: &Output{Text: "out " + path}}
	}
	progress := Block{Kind: ProgressBlock, Steps: []Step{{Text: "Inspect", State: StepInProgress}, {Text: "Report"}}}
	a, b, c := read("a", "a.go"), read("b", "b.go"), read("c", "c.go")
	d := Call{ID: "d", Name: "command", Label: "Command", Detail: "make", Code: true}
	running := func(call Call) Call { call.Running, call.Output = true, nil; return call }
	awaiting := func(call Call) Call { call.AwaitingModel = true; return call }
	failed := func(call Call) Call { call.Failed, call.Error = true, "exit 2"; return call }
	steps := [][]Block{
		callBlocks(a, running(b)),
		append(callBlocks(a, awaiting(b)), progress),
		append(append(callBlocks(a, b), progress), callBlocks(running(c))...),
		append(append(callBlocks(a, b), progress), callBlocks(awaiting(c), failed(d))...),
	}
	plan := i18n.In("en", "task_progress")
	latest := []Call{b, b, c, d}
	s := New()
	turns := sweepTurn(ExecutingTool, steps[0]...)
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { View(c, s, "session", turns, nil, still{}) })
	}, 900, 700)
	tt.SetLocale("en")
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	var header, card ui.Rect
	for i, blocks := range steps {
		turns[0].Reply.Blocks = blocks
		tt.Frame()
		groups := rowButtons(tt, latest[i].Label)
		if len(groups) != 1 || groups[0].Expanded {
			t.Fatalf("step %d: want one closed group, got %+v: %q", i, groups, tt.Texts())
		}
		for _, path := range []string{"a.go", "b.go", "c.go", "make"} {
			if tt.HasText(path) != (path == latest[i].Detail) {
				t.Fatalf("step %d: %q shows outside the closed group: %q", i, path, tt.Texts())
			}
		}
		if i == 0 {
			header = groups[0].Bounds
			continue
		}
		// Its width follows the latest call's summary.
		if g := groups[0].Bounds; g.X != header.X || g.Y != header.Y || g.H != header.H {
			t.Fatalf("step %d: the group moved from %v to %v", i, header, g)
		}
		r, ok := tt.Find(plan)
		if !ok || r.Y <= header.Y {
			t.Fatalf("step %d: the progress card is not below the group: %q", i, tt.Texts())
		}
		if card.H == 0 {
			card = r
		} else if r != card {
			t.Fatalf("step %d: the progress card moved from %v to %v", i, card, r)
		}
	}
	flatRowsShot(t, tt, "streaming-closed")
	if err := tt.Click("make"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	// Below the header, which repeats the latest call, the rows keep the run's order.
	texts := tt.Texts()
	rows := texts[slices.Index(texts, "make")+1:]
	last := -1
	for _, path := range []string{"a.go", "b.go", "c.go", "make"} {
		at := slices.Index(rows, path)
		if at <= last {
			t.Fatalf("%q is missing or out of order in the open group: %q", path, texts)
		}
		last = at
	}
	flatRowsShot(t, tt, "streaming-open")
}

// A call joining an open group comes in below the others, growing from
// nothing; while the system asks for less motion it is whole at once.
func TestToolJoiningOpenGroupEnters(t *testing.T) {
	for _, reduce := range []bool{false, true} {
		calls := []Call{
			{ID: "a", Name: "read", Label: "Read file", Detail: "a.go", Code: true},
			{ID: "b", Name: "read", Label: "Read file", Detail: "b.go", Code: true},
		}
		s := New()
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			s.tools(c, calls, true)
		}, 600, 400)
		tt.SetLocale("en")
		tt.SetPreferences(ui.Preferences{ReduceMotion: reduce})
		tt.Frame()
		if err := tt.Click("b.go"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		before, _ := tt.Find("a.go")
		// d follows c, so the header repeats d and c.go names c's row alone.
		calls = append(calls, Call{ID: "c", Name: "grep", Label: "Search", Detail: "c.go", Code: true, Running: true},
			Call{ID: "d", Name: "grep", Label: "Search", Detail: "d.go", Code: true, Running: true})
		tt.Frame()
		tt.Advance(30 * time.Millisecond)
		tt.Frame()
		// Find reports the row's text as its clip shows it.
		mid, _ := tt.Find("c.go")
		if a, _ := tt.Find("a.go"); a != before {
			t.Fatalf("reduce %v: an earlier row moved from %v to %v", reduce, before, a)
		}
		tt.Advance(time.Second)
		tt.Frame()
		end, ok := tt.Find("c.go")
		if !ok || end.Y <= before.Y || end.H != before.H {
			t.Fatalf("reduce %v: the new call is not a whole row below the others at %v: %q", reduce, end, tt.Texts())
		}
		if growing := mid.H < end.H; growing == reduce {
			t.Fatalf("reduce %v: the new row was %v high during the entry and %v after it", reduce, mid.H, end.H)
		}
	}
}

func TestToolGroupScrollFollowsLatestUntilScrolledAway(t *testing.T) {
	s := New()
	var calls []Call
	appendTool := func() {
		id := fmt.Sprintf("tool-%02d", len(calls)+1)
		calls = append(calls, Call{ID: id, Name: "read", Label: "Read file", Detail: id, Code: true})
	}
	for range 8 {
		appendTool()
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Scroll(c.Key("transcript")).Fill().Children(func() {
			s.tools(c, calls, true)
			ui.Box(c.Key("tail")).Height(600)
		})
	}, 600, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tt.SetLocale("en")
	if err := tt.Click("tool-08"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	visible := func(name string) (ui.Rect, bool) {
		bounds, found := tt.Find(name)
		return bounds, found && bounds.W > 0 && bounds.H > 0
	}
	shown := func(name string) bool {
		_, ok := visible(name)
		return ok
	}
	assertBottom := func(stage string) {
		t.Helper()
		first := len(calls) - 5
		// The header repeats the latest call. Check the other visible rows
		// and the one immediately above the viewport.
		for _, call := range calls[first : len(calls)-1] {
			if !shown(call.Detail) {
				t.Fatalf("%s: %q is outside the viewport at the bottom: %q", stage, call.Detail, tt.Texts())
			}
		}
		if previous := calls[first-1].Detail; shown(previous) {
			t.Fatalf("%s: %q is visible above the latest five rows: %q", stage, previous, tt.Texts())
		}
	}
	assertBottom("open")
	row, _ := visible("tool-04")
	appendTool()
	tt.Frame()
	assertBottom("first append")
	x, y := row.X+row.W/2, row.Y+row.H/2
	tt.Scroll(x, y, 0, -64)
	anchored, ok := visible("tool-04")
	if !ok || anchored.Y <= row.Y {
		t.Fatalf("scrolling did not reveal earlier tools inside the group: %q", tt.Texts())
	}
	appendTool()
	tt.Frame()
	if got, ok := visible("tool-04"); !ok || got != anchored || shown("tool-09") {
		t.Fatalf("an appended tool moved the manually scrolled group: %v, want %v; %q", got, anchored, tt.Texts())
	}
	tt.Scroll(x, y, 0, 160)
	assertBottom("scroll back down")
	appendTool()
	tt.Frame()
	assertBottom("append after scrolling back down")
}

func TestFailedCommandStaysFoldedAfterStop(t *testing.T) {
	const failure = "exit status 2"
	s := New()
	call := Call{ID: "command", Name: "run_command", Label: "Command", Detail: "make", Code: true, Standalone: true, Failed: true, Output: &Output{Failure: failure, Command: true}}
	turns := []Turn{{ID: "run", User: "Build", Reply: Reply{Blocks: []Block{{Kind: ToolsBlock, Calls: []Call{call}}}}}}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		View(c, s, "session", turns, nil, still{})
	}, 900, 700)
	tt.SetLocale("en")
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	for _, status := range []RunStatus{Running, Stopping, Cancelled} {
		turns[0].Reply.Status = status
		tt.Frame()
		if !tt.HasText(call.Label) || !tt.HasText(call.Detail) || tt.HasText(failure) {
			t.Fatalf("status %d did not show just the command summary: %q", status, tt.Texts())
		}
	}
	if err := tt.Click(call.Label); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText(call.Detail) || !tt.HasText(failure) {
		t.Fatalf("opening the command lost its input or failure: %q", tt.Texts())
	}
}

// Expanding a tool group or one of its rows must settle its exact height on
// the click frame so hovering over the rows never causes the group or content
// below it to jump.
func TestToolGroupExpandNoHoverJitter(t *testing.T) {
	for _, count := range []int{2, 3, 4, 5, 6, 8} {
		for _, size := range []uint8{12, 14, 18} {
			t.Run(fmt.Sprintf("calls_%d_size_%d", count, size), func(t *testing.T) {
				s := New()
				calls := make([]Call, count)
				for i := range count {
					detail := fmt.Sprintf("dir-%02d", i+1)
					calls[i] = Call{
						ID:     detail,
						Name:   "list_directory",
						Label:  "List directory",
						Icon:   "folder",
						Detail: detail,
						Code:   true,
						Output: &Output{Text: "line1\nline2\nline3\nline4"},
					}
				}
				const marker = "below-group-marker"
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Dark)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					ui.Column(c).FillWidth().Gap(8).Children(func() {
						s.tools(c, calls, false)
						ui.Text(c, marker).FontSize(float32(size))
					})
				}, 700, 600)
				tt.SetLocale("en")
				tt.Frame()

				// Click the folded group header to expand it (single frame after click).
				latest := fmt.Sprintf("dir-%02d", count)
				if err := tt.Click(latest); err != nil {
					t.Fatal(err)
				}
				tt.Frame()

				afterExpand, ok := tt.Find(marker)
				if !ok {
					t.Fatalf("marker below group disappeared after expand: %q", tt.Texts())
				}
				firstVisible := fmt.Sprintf("dir-%02d", max(1, count-4))
				rowBox, ok := tt.Find(firstVisible)
				if !ok {
					t.Fatalf("expected row %q after expand: %q", firstVisible, tt.Texts())
				}

				// Move pointer over the expanded row and rebuild (simulating hover).
				tt.Move(rowBox.X+rowBox.W/2, rowBox.Y+rowBox.H/2)
				tt.Frame()
				afterHover, _ := tt.Find(marker)
				if afterHover != afterExpand {
					t.Fatalf("hover after group expand moved content below from %v to %v", afterExpand, afterHover)
				}

				// For short groups, opening and closing an inner row must also
				// settle its height on the click frame without shifting on hover.
				if count == 2 {
					if err := tt.Click("dir-01"); err != nil {
						t.Fatal(err)
					}
					tt.Frame()
					afterRowOpen, _ := tt.Find(marker)
					if afterRowOpen.Y <= afterExpand.Y {
						t.Fatalf("expanding inner row did not push content below down: before=%v after=%v", afterExpand, afterRowOpen)
					}
					tt.Move(afterRowOpen.X+4, afterRowOpen.Y+4)
					tt.Frame()
					if afterRowHover, _ := tt.Find(marker); afterRowHover != afterRowOpen {
						t.Fatalf("hover after inner row expand moved content below from %v to %v", afterRowOpen, afterRowHover)
					}
					if err := tt.Click("dir-01"); err != nil {
						t.Fatal(err)
					}
					tt.Frame()
					afterRowClose, _ := tt.Find(marker)
					if afterRowClose != afterExpand {
						t.Fatalf("collapsing inner row did not restore height immediately: got=%v want=%v", afterRowClose, afterExpand)
					}
					tt.Move(rowBox.X+rowBox.W/2, rowBox.Y+rowBox.H/2)
					tt.Frame()
					if afterCloseHover, _ := tt.Find(marker); afterCloseHover != afterExpand {
						t.Fatalf("hover after inner row collapse moved content below from %v to %v", afterExpand, afterCloseHover)
					}
				}
			})
		}
	}
}

func TestRepeatedToolRowsCollapse(t *testing.T) {
	t.Run("homogeneous repeated calls collapse without double nesting", func(t *testing.T) {
		s := New()
		calls := make([]Call, 5)
		for i := range calls {
			calls[i] = Call{
				ID: fmt.Sprintf("rc-%d", i), Name: "read_review_change", Label: "Read review change", Icon: "code",
				Output: &Output{Text: fmt.Sprintf("diff chunk %d", i)},
			}
		}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			s.tools(c, calls, false)
		}, 700, 500)
		tt.SetLocale("en")
		tt.SetPreferences(ui.Preferences{ReduceMotion: true})
		tt.Frame()
		if textCount(tt, "Read review change") != 1 || !tt.HasText("×5") {
			t.Fatalf("5 identical calls should collapse into a single ×5 row: %q", tt.Texts())
		}
		if err := tt.Click("Read review change"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		// Opening the repeat row reveals the 5 individual calls (plus header = 6).
		if got := textCount(tt, "Read review change"); got != 6 {
			t.Fatalf("opening ×5 row showed %d headers, want 6: %q", got, tt.Texts())
		}
		clickRow(t, tt, "Read review change")
		tt.Frame()
		if !tt.HasText("diff chunk 4") {
			t.Fatalf("opening an individual call inside a collapsed repeat did not show its output: %q", tt.Texts())
		}
	})

	t.Run("all-bare repeated calls stay a single plain row", func(t *testing.T) {
		s := New()
		calls := make([]Call, 4)
		for i := range calls {
			calls[i] = Call{ID: fmt.Sprintf("bare-%d", i), Name: "ping", Label: "Check status", Icon: "code"}
		}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			s.tools(c, calls, false)
		}, 700, 400)
		tt.SetLocale("en")
		tt.Frame()
		if textCount(tt, "Check status") != 1 || !tt.HasText("×4") || len(rowButtons(tt, "Check status")) != 0 {
			t.Fatalf("bare repeated calls should render a non-expandable ×4 row: %q", tt.Texts())
		}
	})

	t.Run("mixed group collapses repeated middle run and keeps warning distinct", func(t *testing.T) {
		s := New()
		calls := []Call{
			{ID: "scope", Name: "review_scope", Label: "Review scope", Icon: "code", Warning: true, Output: &Output{Failure: "invalid arguments", ToolFailure: true}},
			{ID: "r1", Name: "read_change", Label: "Read review change", Icon: "code", Output: &Output{Text: "chunk 1"}},
			{ID: "r2", Name: "read_change", Label: "Read review change", Icon: "code", Output: &Output{Text: "chunk 2"}},
			{ID: "r3", Name: "read_change", Label: "Read review change", Icon: "code", Output: &Output{Text: "chunk 3"}},
			{ID: "r4", Name: "read_change", Label: "Read review change", Icon: "code", Warning: true, Output: &Output{Failure: "chunk error", ToolFailure: true}},
			{ID: "r5", Name: "read_change", Label: "Read review change", Icon: "code", Output: &Output{Text: "chunk 5"}},
			{ID: "done", Name: "report", Label: "Report finding", Icon: "code", Output: &Output{Text: "recorded"}},
		}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			s.tools(c, calls, false)
		}, 700, 600)
		tt.SetLocale("en")
		tt.SetPreferences(ui.Preferences{ReduceMotion: true})
		tt.Frame()
		if err := tt.Click("Report finding"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		// Inside the open group: Review scope, Read review change ×3, Read review change (warning), Read review change (single), Report finding.
		if !tt.HasText("×3") || textCount(tt, "Read review change") != 3 {
			t.Fatalf("expected one ×3 repeat, one distinct warning call, and one trailing single call: %q", tt.Texts())
		}
		if err := tt.Click("×3"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if got := textCount(tt, "Read review change"); got != 6 {
			t.Fatalf("opening ×3 inside mixed group showed %d Read review change labels, want 6: %q", got, tt.Texts())
		}
	})
}
