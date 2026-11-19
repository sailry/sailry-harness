package conversation

import (
	"reflect"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestEarlierReplySegmentFoldsWhileTurnContinues(t *testing.T) {
	state := New()
	turns := []Turn{{
		ID: "turn",
		Segments: []Segment{
			{Inputs: []Input{{ID: "turn", User: "Inspect the code"}}},
			{From: 2, Inputs: []Input{{ID: "guide", User: "Check the test too"}}},
		},
		Reply: Reply{Status: Running, Blocks: []Block{
			{Kind: ToolsBlock, Calls: []Call{{ID: "earlier", Label: "Read file", Detail: "history.go"}}},
			{Kind: TextBlock, Text: "Earlier answer"},
			{Kind: ToolsBlock, Calls: []Call{{ID: "latest", Label: "Read file", Detail: "active_test.go"}}},
			{Kind: TextBlock, Text: "Latest answer"},
		}},
	}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, state, "session", turns, nil, still{}) })
	}, 900, 800)
	tester.SetLocale("zh-CN")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	completed := i18n.In("zh-CN", "turn_completed")
	for _, status := range []RunStatus{Running, Continued} {
		turns[0].Reply.Status = status
		tester.Frame()
		button, ok := tester.AccessByRole(ui.RoleButton, completed)
		if !ok || button.Expanded || tester.HasText("history.go") || !tester.HasText("Earlier answer") || !tester.HasText("active_test.go") {
			t.Fatalf("%s: earlier work should be folded with the latest work visible: %q", StatusKey(status), tester.Texts())
		}
	}
	if err := tester.Click(completed); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("history.go") {
		t.Fatalf("opening the completed segment did not reveal earlier work: %q", tester.Texts())
	}
}

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
