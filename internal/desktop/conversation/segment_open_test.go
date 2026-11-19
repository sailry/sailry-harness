package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// A turn that did not complete opens every part of its work by default,
// including the parts before consumed guidance; a completed one stays folded.
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
		if tt.HasText("a.go") != open || tt.HasText("b.go") != open {
			t.Fatalf("status %d: work open %v, want %v: %q", status, tt.HasText("a.go"), open, tt.Texts())
		}
	}
}
