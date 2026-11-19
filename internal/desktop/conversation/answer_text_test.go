package conversation

import (
	"strings"
	"testing"
)

func TestReplyTextSkipsWorkAcrossSegments(t *testing.T) {
	blocks := []Block{
		{Kind: TextBlock, Text: "work"}, {Kind: ErrorBlock, Text: "error"},
		{Kind: ToolsBlock, Calls: []Call{{Name: "read"}, {Terminal: true}}},
		{Kind: TextBlock, Text: "first"}, {Kind: TextBlock, Text: "   "},
		{Kind: ChildrenBlock}, {Kind: TextBlock, Text: "second"},
		{Kind: ErrorBlock}, {Kind: TextBlock, Text: "third"},
	}
	turn := Turn{Reply: Reply{Blocks: blocks}}
	if got := replyText(&turn); got != "second\n\nthird" {
		t.Fatalf("single answer = %q", got)
	}
	turn.Segments = []Segment{{From: 0}, {From: 5}}
	if got := replyText(&turn); got != "first\n\nsecond\n\nthird" {
		t.Fatalf("segmented answer = %q", got)
	}
}

func BenchmarkToolHeavyReplyText(b *testing.B) {
	blocks := make([]Block, 400)
	for i := range blocks {
		blocks[i] = Block{Kind: ToolsBlock, Calls: []Call{{Name: "read_file"}}}
	}
	blocks = append(blocks, Block{Kind: TextBlock, Text: strings.Repeat("answer ", 100)})
	turn := Turn{Reply: Reply{Blocks: blocks}}
	b.ReportAllocs()
	for b.Loop() {
		if replyText(&turn) == "" {
			b.Fatal("missing answer")
		}
	}
}
