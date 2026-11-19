package conversation

import (
	"reflect"
	"slices"
	"testing"
)

func TestFoldedReplyMatchesExpandedBoundaries(t *testing.T) {
	samples := []Block{
		{Kind: TextBlock, Text: "answer"}, {Kind: TextBlock, Text: "  "},
		{Kind: ErrorBlock, Text: "problem"}, {Kind: QuestionBlock}, {Kind: RetryBlock},
		{Kind: ToolsBlock}, {Kind: ToolsBlock, Calls: []Call{{Terminal: true}}},
		{Kind: ToolsBlock, Calls: []Call{{Terminal: true}, {Name: "read"}, {Name: "stop", Terminal: true}}},
		{Kind: ChildrenBlock, Calls: []Call{{Name: "spawn"}}, Children: []Child{{}}},
	}
	for _, a := range samples {
		for _, b := range samples {
			for _, c := range samples {
				blocks := []Block{a, b, c}
				full := Compact(blocks)
				start, end := AnswerFrom(full), len(full)
				for end > 0 && full[end-1].Kind == ErrorBlock {
					end--
				}
				equalBlock := func(a, b Block) bool { return reflect.DeepEqual(a, b) }
				for _, expanded := range []bool{true, false} {
					got := partitionReply(blocks, expanded)
					if got.hasWork != (start > 0) || got.endsInRetry != (len(full) > 0 && full[len(full)-1].Kind == RetryBlock) ||
						!slices.EqualFunc(got.answer, full[start:end], equalBlock) || !slices.EqualFunc(got.failures, full[end:], equalBlock) ||
						(expanded && !slices.EqualFunc(got.work, full[:start], equalBlock)) || (!expanded && len(got.work) != 0) {
						t.Fatalf("expanded=%t, input=%#v, sections=%#v", expanded, blocks, got)
					}
				}
			}
		}
	}
}

func BenchmarkPartitionToolHeavyReply(b *testing.B) {
	blocks := make([]Block, 400)
	for i := range blocks {
		blocks[i] = Block{Kind: ToolsBlock, Calls: []Call{{Name: "read_file"}}}
	}
	blocks = append(blocks, Block{Kind: TextBlock, Text: "answer"})
	for _, setup := range []struct {
		name     string
		expanded bool
	}{{"folded", false}, {"expanded", true}} {
		b.Run(setup.name, func(b *testing.B) {
			b.ReportAllocs()
			for b.Loop() {
				if len(partitionReply(blocks, setup.expanded).answer) != 1 {
					b.Fatal("missing answer")
				}
			}
		})
	}
}
