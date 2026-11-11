package conversation

import (
	"fmt"
	"image"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func named(name string, n int) []Call {
	out := make([]Call, n)
	for i := range out {
		out[i] = Call{ID: fmt.Sprintf("%s%d", name, i), Name: name}
	}
	return out
}

// TestGrouping groups the pure grouping and summary rules; each case keeps its own fixture.
func TestGrouping(t *testing.T) {
	t.Run("Compact", testCompact)
	t.Run("RunsAndSequence", testRunsAndSequence)
	t.Run("Summary", testSummary)
	t.Run("AnswerFrom", testAnswerFrom)
	t.Run("LastLines", testLastLines)
	t.Run("Elapsed", testElapsed)
	t.Run("Disclosures", testDisclosures)
	t.Run("TextSize", testTextSize)
	t.Run("UsageSize", testUsageSize)
}

// The context bar's figures and their cards do not follow the message text
// size: at 12, 14 and 20 a figure is as wide as its text measures at 14, in
// the bar and in the compact button, and a card's value, in tabular figures,
// is the same at each.
func testUsageSize(t *testing.T) {
	const value, label, detail = "48.2k", "Tokens", "41.0k"
	figures := [][]Metric{{{ID: "tokens", Label: label, Value: value, Rows: []Row{{Label: "This session", Value: detail}}}}}
	var card ui.Rect
	for _, size := range []uint8{12, 14, 20} {
		for _, compact := range []bool{false, true} {
			var want float32
			open := false
			tt := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				kit.SetPreferences(c, kit.Preferences{MessageTextSize: size})
				want, _ = c.MeasureText(0, ui.Span{Text: value, Size: 14})
				ui.Column(c).Fill().AlignItems(ui.End).Padding(40).Children(func() {
					ui.Spacer(c)
					UsageBar(c, nil, figures, compact, &open)
				})
			}, 600, 400)
			tt.SetLocale("en")
			tt.Frame()
			figure, ok := tt.Find(value)
			if !ok || math.Abs(float64(figure.W-want)) > 1 {
				t.Fatalf("size %d, compact %v: the figure is %v wide, its text %v at 14: %q", size, compact, figure.W, want, tt.Texts())
			}
			tt.Move(figure.X+figure.W/2, figure.Y+figure.H/2)
			tt.Advance(time.Second)
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
			line, ok := tt.Find(detail)
			if card.W == 0 {
				card = line
			}
			if !ok || line.W != card.W || line.H != card.H {
				t.Errorf("size %d, compact %v: the card's value is %v by %v, at 12 %v by %v: %q", size, compact, line.W, line.H, card.W, card.H, tt.Texts())
			}
		}
	}
}

// ink measures what is drawn in r of img, a frame at scale, in DIPs: the
// height of the middle of its ink by weight, the capitals' of a text of
// capitals, and the heights of its first and last rows with ink, an icon's
// outline. Ink is how far a pixel is from the color 3 DIPs before r.
func ink(t *testing.T, img *image.RGBA, scale float32, r ui.Rect) (centre, top, bottom float64) {
	t.Helper()
	bg := img.RGBAAt(int((r.X-3)*scale), int((r.Y+r.H/2)*scale))
	x0, x1 := int(r.X*scale), int(math.Ceil(float64((r.X+r.W)*scale)))
	y0, y1 := int(math.Ceil(float64(r.Y*scale))), int((r.Y+r.H)*scale)
	var rows []float64
	var sum, weighted, peak float64
	for y := y0; y < y1; y++ {
		v := 0.0
		for x := x0; x < x1; x++ {
			p := img.RGBAAt(x, y)
			v += math.Abs(float64(p.R)-float64(bg.R)) + math.Abs(float64(p.G)-float64(bg.G)) + math.Abs(float64(p.B)-float64(bg.B))
		}
		rows = append(rows, v)
		sum, weighted, peak = sum+v, weighted+v*(float64(y)+0.5), max(peak, v)
	}
	if peak == 0 {
		t.Fatalf("nothing is drawn in %v", r)
	}
	first, last := -1, -1
	for i, v := range rows {
		if v > peak/20 {
			if first < 0 {
				first = i
			}
			last = i
		}
	}
	return weighted / sum / float64(scale), float64(y0+first) / float64(scale), float64(y0+last+1) / float64(scale)
}

// The transcript's rows follow the message text size (textSize). At 12, 14
// and 18 a call's label is the size of the user message's text; its row is
// 10 DIPs higher than the text and its icon 2 larger, the reference's 24
// and 16 at 14, with the icon's middle within a DIP of the label's
// capitals' and of the row's; the status header is as high and the phase's
// mark as large; and no row reaches the next.
func testTextSize(t *testing.T) {
	const user, first, second, between = "CHECK IT", "RUN CHECKS", "READ FILES", "Between"
	const scale = 2
	working := i18n.In("en", "chat_running")
	outline := map[float32]float64{} // the first row's icon, top to bottom
	for _, size := range []float32{12, 14, 18} {
		s := New()
		turns := []Turn{{ID: "turn", User: user, Reply: Reply{Status: Running, Blocks: []Block{
			{Kind: ToolsBlock, Calls: []Call{{ID: "a", Name: "shell", Label: first, Icon: "circle-check", Detail: "MAKE TEST", Code: true, Output: &Output{Text: "ok"}}}},
			{Kind: TextBlock, Text: between},
			{Kind: ToolsBlock, Calls: []Call{{ID: "b", Name: "read", Label: second, Icon: "file-text", Output: &Output{Text: "ok"}}}},
		}}}}
		var userWidth, labelWidth float32
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			kit.SetPreferences(c, kit.Preferences{MessageTextSize: uint8(size)})
			userWidth, _ = c.MeasureText(0, ui.Span{Text: user, Size: size})
			labelWidth, _ = c.MeasureText(0, ui.Span{Text: first, Size: size})
			ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { View(c, s, "session", turns, nil, still{}) })
		}, 900, 600)
		tt.SetLocale("en")
		tt.SetScale(scale)
		tt.SetPreferences(ui.Preferences{ReduceMotion: true}) // no band over the running row
		tt.Frame()

		bubble, ok := tt.Find(user)
		label, ok2 := tt.Find(first)
		other, ok3 := tt.Find(second)
		text, ok4 := tt.Find(between)
		header, ok5 := tt.AccessByRole(ui.RoleButton, working)
		if !ok || !ok2 || !ok3 || !ok4 || !ok5 {
			t.Fatalf("size %v: the turn shows %q", size, tt.Texts())
		}
		// A call's row is the smallest button around its label; the phase's
		// mark is the progress indicator below the rows.
		var rows [2]ui.Rect
		var mark ui.Rect
		for _, a := range tt.AccessAll() {
			b := a.Bounds
			for i, l := range []ui.Rect{label, other} {
				middle := l.Y + l.H/2
				if a.Role == ui.RoleButton && b.X <= l.X && b.X+b.W >= l.X+l.W && b.Y <= middle && b.Y+b.H >= middle && (rows[i].H == 0 || b.H < rows[i].H) {
					rows[i] = b
				}
			}
			if a.Role == ui.RoleProgress && b.Y > other.Y {
				mark = b
			}
		}
		if rows[0].H == 0 || rows[1].H == 0 {
			t.Fatalf("size %v: no button around %q at %v or %q at %v", size, first, label, second, other)
		}

		// The label is the body's size: as wide as its text measures at it, as
		// the user's bubble is around its padding of 14, their capitals as high.
		if d := float64(label.W - labelWidth); math.Abs(d) > 1 {
			t.Errorf("size %v: the label is %v wide, its text %v at that size", size, label.W, labelWidth)
		}
		if d := float64(bubble.W - 28 - userWidth); math.Abs(d) > 1 {
			t.Errorf("size %v: the user message is %v wide, its text %v at that size and 28 of padding", size, bubble.W, userWidth)
		}
		img := tt.Image()
		_, bodyTop, bodyBottom := ink(t, img, scale, ui.Rect{X: bubble.X + 14, Y: bubble.Y, W: bubble.W - 28, H: bubble.H})
		_, capTop, capBottom := ink(t, img, scale, ui.Rect{X: label.X, Y: rows[0].Y, W: label.W, H: rows[0].H})
		if body, caps := bodyBottom-bodyTop, capBottom-capTop; math.Abs(body-caps) > 1 {
			t.Errorf("size %v: the label's capitals are %.2f high, the user message's %.2f", size, caps, body)
		}

		// The rows' metrics, and the header's and the mark's.
		height, icon := size+10, size+2
		if rows[0].H != height || rows[1].H != height || header.Bounds.H != height {
			t.Errorf("size %v: the rows are %v and %v high and the header %v, want %v", size, rows[0].H, rows[1].H, header.Bounds.H, height)
		}
		if mark.H != icon {
			t.Errorf("size %v: the phase's mark is %v high, want %v", size, mark.H, icon)
		}
		for i, l := range []ui.Rect{label, other} {
			row := rows[i]
			// The icon is the size+2 DIPs before the label's 8 DIP gap.
			_, top, bottom := ink(t, img, scale, ui.Rect{X: l.X - 8 - icon, Y: row.Y, W: icon, H: row.H})
			caps, _, _ := ink(t, img, scale, ui.Rect{X: l.X, Y: row.Y, W: l.W, H: row.H})
			middle, centre := (top+bottom)/2, float64(row.Y+row.H/2)
			if i == 0 {
				outline[size] = bottom - top
			}
			t.Logf("size %v, row %d: centre %.2f, icon %+.2f, label %+.2f", size, i, centre, middle-centre, caps-centre)
			if math.Abs(caps-middle) > 1 || math.Abs(middle-centre) > 1 {
				t.Errorf("size %v, row %d: the icon's middle is at %.2f, the label's capitals' at %.2f and the row's at %.2f", size, i, middle, caps, centre)
			}
		}

		// Top to bottom, none reaches the next.
		order := []ui.Rect{header.Bounds, rows[0], text, rows[1], mark}
		for i := 1; i < len(order); i++ {
			if above, below := order[i-1], order[i]; above.Y+above.H > below.Y+0.01 {
				t.Errorf("size %v: row %d ends at %v, past the next one's top at %v", size, i-1, above.Y+above.H, below.Y)
			}
		}
	}
	// The icon is drawn 2 DIPs larger than the text: its outline scales
	// with it from the one at 14.
	for _, size := range []float32{12, 18} {
		if want := outline[14] * float64(size+2) / 16; outline[14] == 0 || math.Abs(outline[size]-want) > 1 {
			t.Errorf("size %v: the icon is %.2f high, want %.2f as %.2f at 14", size, outline[size], want, outline[14])
		}
	}
}

// still is the transcript's actions for a test that asks none: it answers
// the one a frame binds before any is asked, the image viewer's.
type still struct{ Actions }

func (still) ImageAction(kit.ImageAction, Image) {}

// A call's row opens to its output and closes again; the state keeps the
// open rows alone, none for the rows a transcript only shows.
func testDisclosures(t *testing.T) {
	s := New()
	turns := []Turn{{ID: "turn", User: "Check", Reply: Reply{Status: Running, Blocks: []Block{
		{Kind: ToolsBlock, Calls: []Call{{ID: "call", Name: "shell", Label: "Run the checks", Output: &Output{Text: "all passed"}}}},
		{Kind: ReasoningBlock, Text: "Next"},
	}}}}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		View(c, s, "session", turns, nil, still{})
	}, 900, 600)
	tt.Frame()
	if !tt.HasText("Run the checks") || tt.HasText("all passed") || len(s.open) != 0 {
		t.Fatalf("closed rows: %q, %d kept", tt.Texts(), len(s.open))
	}
	if err := tt.Click("Run the checks"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("all passed") || len(s.open) != 1 {
		t.Fatalf("the open row: %q, %d kept", tt.Texts(), len(s.open))
	}
	if err := tt.Click("Run the checks"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if tt.HasText("all passed") || len(s.open) != 0 {
		t.Errorf("the row closed again: %q, %d kept", tt.Texts(), len(s.open))
	}
}

// Adjacent tool blocks join, text splits them and blank text does not;
// terminal calls stand alone; adjacent children join.
func testCompact(t *testing.T) {
	t.Run("citations retain Unicode positions across work and answer blocks", func(t *testing.T) {
		blocks := []Block{{Kind: TextBlock, Text: "结果🙂"}, {Kind: ToolsBlock, Calls: named("read", 1)}, {Kind: TextBlock, Text: "More"}}
		citations := []Citation{{URI: "https://example.com/start", End: -1}, {URI: "https://example.com/first", End: 3}, {URI: "https://example.com/next-start", End: -4}, {URI: "https://example.com/second", End: 5}, {URI: "https://example.com/final"}}
		got := cited(blocks, citations)
		if got[0].Text != " [1](<https://example.com/start>)结果🙂 [2](<https://example.com/first>)" || got[2].Text != " [3](<https://example.com/next-start>)Mo [4](<https://example.com/second>)re [5](<https://example.com/final>)" {
			t.Fatal(got)
		}
		if blocks[0].Text != "结果🙂" || blocks[2].Text != "More" || AnswerText(Compact(blocks)) != "More" || AnswerFrom(Compact(got)) != 2 {
			t.Fatal("citation rendering changed canonical copy or answer grouping", blocks)
		}
	})
	read, grep := named("read", 2), named("grep", 1)
	ask := Call{ID: "ask", Name: "ask_user", Terminal: true}
	got := Compact([]Block{
		{Kind: ToolsBlock, Calls: read},
		{Kind: TextBlock, Text: "  \n"},
		{Kind: ToolsBlock, Calls: grep},
		{Kind: ToolsBlock, Calls: []Call{ask}},
		{Kind: TextBlock, Text: "answer"},
		{Kind: ToolsBlock, Calls: named("edit", 1)},
		{Kind: ChildrenBlock, Children: []Child{{ID: "a"}}},
		{Kind: ChildrenBlock, Children: []Child{{ID: "b"}}},
	})
	kinds := []BlockKind{ToolsBlock, ToolsBlock, TextBlock, ToolsBlock, ChildrenBlock}
	if len(got) != len(kinds) {
		t.Fatalf("%d blocks: %+v", len(got), got)
	}
	for i, k := range kinds {
		if got[i].Kind != k {
			t.Errorf("block %d is %d, want %d", i, got[i].Kind, k)
		}
	}
	if len(got[0].Calls) != 3 || len(got[1].Calls) != 1 || len(got[4].Children) != 2 {
		t.Errorf("not joined: %d calls, %d terminal, %d children", len(got[0].Calls), len(got[1].Calls), len(got[4].Children))
	}
}

func testRunsAndSequence(t *testing.T) {
	calls := append(append(named("read", 2), named("grep", 1)...), named("read", 1)...)
	runs := Runs(calls)
	if len(runs) != 3 || len(runs[0]) != 2 {
		t.Fatalf("runs %v", runs)
	}
	if !Sequence(calls) || Sequence(named("read", 3)) || Sequence(named("read", 1)) {
		t.Error("a sequence is more than one call of more than one name")
	}
}

// The header of a group is its last call with the flags of all of them:
// failures show only once nothing runs, and a failure hides warnings.
func testSummary(t *testing.T) {
	calls := named("read", 3)
	calls[0].Failed, calls[1].Warning, calls[2].Detail = true, true, "last"
	s := Summary(calls)
	if s.Detail != "last" || !s.Failed || s.Warning || s.Output != nil {
		t.Errorf("finished summary %+v", s)
	}
	calls[2].Running, calls[2].State = true, NotExecuted
	if s = Summary(calls); !s.Running || s.Failed || s.State != StateNone {
		t.Errorf("running summary %+v", s)
	}
}

// The work region ends at the last work block; questions and text after
// it are the answer.
func testAnswerFrom(t *testing.T) {
	blocks := []Block{
		{Kind: ReasoningBlock},
		{Kind: ToolsBlock, Calls: named("read", 1)},
		{Kind: TextBlock, Text: "a"},
		{Kind: ToolsBlock, Calls: []Call{{Name: "ask_user", Terminal: true}}},
		{Kind: QuestionBlock},
		{Kind: TextBlock, Text: "b"},
	}
	if got := AnswerFrom(blocks); got != 2 {
		t.Errorf("answer from %d", got)
	}
	if got := AnswerFrom(blocks[2:]); got != 0 {
		t.Errorf("answer from %d without work", got)
	}
	// The notice that the answer was cut stays with the answer.
	if got := AnswerFrom(append(blocks[:6:6], Block{Kind: LimitBlock})); got != 2 {
		t.Errorf("answer from %d with a cut answer", got)
	}
}

func testLastLines(t *testing.T) {
	var lines []string
	for i := range 130 {
		lines = append(lines, fmt.Sprint(i))
	}
	text, hidden := lastLines(strings.Join(lines, "\n") + "\n")
	if hidden != 30 || !strings.HasPrefix(text, "30\n") || !strings.HasSuffix(text, "129") {
		t.Errorf("hidden %d, text starts %q", hidden, text[:4])
	}
}

func testElapsed(t *testing.T) {
	for d, want := range map[int]string{9: "9s", 65: "1m05s", 3725: "1h02m05s"} {
		if got := Elapsed(secs(d)); got != want {
			t.Errorf("%d s is %q, want %q", d, got, want)
		}
	}
}

func secs(n int) time.Duration { return time.Duration(n) * time.Second }
