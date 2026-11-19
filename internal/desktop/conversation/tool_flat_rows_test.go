package conversation

import (
	"cmp"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

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

func textCount(tt *ui.Tester, text string) int {
	n := 0
	for _, s := range tt.Texts() {
		if s == text {
			n++
		}
	}
	return n
}

// A block's calls, of one name or several, are one row each in their order,
// none a summary of others: one click opens a row straight onto its result,
// at each supported text size and at a narrow and a wide window.
func TestToolRowsAreFlat(t *testing.T) {
	calls := []Call{
		{ID: "r1", Name: "read", Label: "Read file", Icon: "file-text", Detail: "a.go", Code: true, Output: &Output{Text: "out a"}},
		{ID: "r2", Name: "read", Label: "Read file", Icon: "file-text", Detail: "b.go", Code: true, Output: &Output{Text: "out b"}},
		{ID: "g1", Name: "grep", Label: "Search", Icon: "search", Detail: "c.go", Code: true, Output: &Output{Text: "out c"}},
		{ID: "r3", Name: "read", Label: "Read file", Icon: "file-text", Detail: "d.go", Code: true, Output: &Output{Text: "out d"}},
	}
	details := []string{"a.go", "b.go", "c.go", "d.go"}
	for _, size := range []uint8{12, 14, 18} {
		for _, width := range []int{480, 900} {
			t.Run(fmt.Sprintf("size_%d_width_%d", size, width), func(t *testing.T) {
				s := New()
				turns := []Turn{{ID: "turn", User: "Inspect", Reply: Reply{Status: Running, Phase: AwaitingResponse, Blocks: []Block{{Kind: ToolsBlock, Calls: calls}}}}}
				tt := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
					ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { View(c, s, "session", turns, nil, still{}) })
				}, width, 700)
				tt.SetLocale("en")
				tt.SetPreferences(ui.Preferences{ReduceMotion: true})
				tt.Frame()
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
				buttons := append(rowButtons(tt, "Read file"), rowButtons(tt, "Search")...)
				if len(buttons) != len(calls) {
					t.Fatalf("%d tool row buttons, want one per call: %+v", len(buttons), buttons)
				}
				for _, b := range buttons {
					if b.Bounds.X != buttons[0].Bounds.X || b.Expanded {
						t.Fatalf("row %q at %v is not a closed row on the first one's level %v", b.Name, b.Bounds, buttons[0].Bounds)
					}
				}
				flatRowsShot(t, tt, fmt.Sprintf("closed-%d-%d", size, width))
				if err := tt.Click("b.go"); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
				if !tt.HasText("out b") || tt.HasText("out a") || tt.HasText("out c") || tt.HasText("out d") || len(s.open) != 1 {
					t.Fatalf("one click did not open its own result alone: %q, %d open", tt.Texts(), len(s.open))
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

// A row listing the files its call touched opens onto their links, once
// each, without the aggregate preview or a Copy action; a failure still
// shows.
func TestToolRowPathList(t *testing.T) {
	s := New()
	var links []string
	s.OpenLink = func(_ *ui.Context, _, link string) bool { links = append(links, link); return true }
	calls := []Call{
		{ID: "find", Name: "find", Label: "Find files", Icon: "search", Output: &Output{Text: "aggregate", Paths: []string{"a/one.go", "a/two.go", "a/one.go"}}},
		{ID: "find2", Name: "find", Label: "Find more", Icon: "search", Output: &Output{Failure: "partial listing", Paths: []string{"b/three.go", "b/four.go"}, Notices: []string{"One folder was skipped"}, Partial: true}},
	}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		s.tools(c, calls, true)
	}, 900, 600)
	tt.SetLocale("en")
	tt.Frame()
	if tt.HasText("a/one.go") || tt.HasText("aggregate") {
		t.Fatalf("closed rows show their paths: %q", tt.Texts())
	}
	if err := tt.Click("Find files"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	// A path row is a button labelled with its path.
	if len(rowButtons(tt, "a/one.go")) != 1 || len(rowButtons(tt, "a/two.go")) != 1 || tt.HasText("aggregate") || tt.HasText("b/three.go") {
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
	if err := tt.Click("Find more"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("b/three.go") || !tt.HasText("partial listing") || !tt.HasText("One folder was skipped") || !tt.HasText(i18n.In("en", "tool_partial")) {
		t.Fatalf("a failed path list: %q", tt.Texts())
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
		{ID: "cmd", Name: "command", Label: "Run", Icon: "square-terminal", Detail: "make", Code: true, Output: &Output{Text: text, Command: true}},
		{ID: "cmd2", Name: "command", Label: "Run", Icon: "square-terminal", Detail: "make test", Code: true, Output: &Output{Text: "ok", Command: true}},
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

// A settled turn's rows stay still, whatever their calls last reported.
func TestHistoryToolRowsStill(t *testing.T) {
	calls := []Call{runningCall("a", "read", "a.go", false), settledCall("b", "grep", "b.go", false, true)}
	turns := []Turn{{ID: "turn", User: "Inspect", Reply: Reply{Status: Completed, Blocks: []Block{{Kind: ToolsBlock, Calls: calls}, {Kind: TextBlock, Text: "Done"}}}}}
	lit := sweeps(t, turns, []string{i18n.In("en", "turn_completed")}, "a.go", "b.go")
	expectSweeps(t, lit, map[string]bool{"a.go": false, "b.go": false})
}
