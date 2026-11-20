package conversation

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

// Parts before consumed guidance stay open; the latest part folds its work
// once completed and opens it when the turn did not complete.
func TestUnfinishedTurnOpensAllParts(t *testing.T) {
	read := func(id, path string) Block {
		return Block{Kind: ToolsBlock, Calls: []Call{{ID: id, Name: "read", Label: "Read file", Detail: path, Code: true, Output: &Output{Text: path}}}}
	}
	for _, status := range []RunStatus{Completed, Cancelled, Interrupted, Failed} {
		turn := Turn{ID: "t", User: "Go", Reply: Reply{Status: status, Blocks: []Block{read("a", "a.go"), {Kind: TextBlock, Text: "said"}, read("b", "b.go")}},
			Segments: []Segment{{From: 0, Inputs: []Input{{ID: "t", User: "Go"}}}, {From: 2, Inputs: []Input{{ID: "m", User: "more"}}}}}
		s := New()
		tt := ui.NewTester(func(c *ui.Context) { View(c, s, "s", []Turn{turn}, nil, still{}) }, 900, 700)
		tt.SetLocale("en")
		tt.Frame()
		open := status != Completed
		if !tt.HasText("a.go") || !tt.HasText("said") || tt.HasText("b.go") != open {
			t.Fatalf("status %d: earlier part %v, latest work %v, want %v: %q", status, tt.HasText("a.go"), tt.HasText("b.go"), open, tt.Texts())
		}
	}
}

// A part answered before consumed guidance keeps its header, plain text
// included, and its open work folds by hand.
func TestEarlierPartKeepsHeader(t *testing.T) {
	read := Block{Kind: ToolsBlock, Calls: []Call{{ID: "a", Name: "read", Label: "Read file", Detail: "a.go", Code: true, Output: &Output{Text: "a.go"}}}}
	for _, blocks := range [][]Block{{{Kind: TextBlock, Text: "hello"}, {Kind: TextBlock, Text: "monday"}}, {read, {Kind: TextBlock, Text: "hello"}, {Kind: TextBlock, Text: "monday"}}} {
		from := len(blocks) - 1
		turn := Turn{ID: "t", User: "Hi", Reply: Reply{Status: Completed, Blocks: blocks},
			Segments: []Segment{{From: 0, Inputs: []Input{{ID: "t", User: "Hi"}}}, {From: from, Inputs: []Input{{ID: "m", User: "day?"}}}}}
		s := New()
		tt := ui.NewTester(func(c *ui.Context) { View(c, s, "s", []Turn{turn}, nil, still{}) }, 900, 700)
		tt.SetLocale("en")
		tt.Frame()
		texts := tt.Texts()
		if header := slices.Index(texts, "Completed"); header < 0 || header > slices.Index(texts, "hello") {
			t.Fatalf("earlier part has no header: %q", texts)
		}
		if from == 2 {
			if !tt.HasText("a.go") {
				t.Fatalf("earlier work folded: %q", tt.Texts())
			}
			s.work["work-t-segment-0"] = false
			tt.Frame()
			if tt.HasText("a.go") {
				t.Fatalf("earlier work did not fold: %q", tt.Texts())
			}
		}
	}
}
