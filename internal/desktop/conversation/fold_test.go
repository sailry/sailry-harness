package conversation

import (
	"fmt"
	"reflect"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The reply before consumed guidance stays open while the turn continues.
func TestEarlierReplySegmentStaysOpenWhileTurnContinues(t *testing.T) {
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
		if !ok || !button.Expanded || !tester.HasText("history.go") || !tester.HasText("Earlier answer") || !tester.HasText("active_test.go") {
			t.Fatalf("%s: earlier work should stay open with the latest work visible: %q", StatusKey(status), tester.Texts())
		}
	}
	if err := tester.Click(completed); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.HasText("history.go") || !tester.HasText("Earlier answer") {
		t.Fatalf("folding the earlier segment did not hide only its work: %q", tester.Texts())
	}
}

func TestCompletedTurnOpensToolResultsOnlyOnRowClick(t *testing.T) {
	const (
		groupA      = "Read source"
		groupB      = "Search fixtures"
		failedRow   = "Inspect workspace"
		warningRow  = "Check fixtures"
		standalone  = "Validate patch"
		failure     = "Workspace inspection failed"
		warning     = "Fixture validation rejected"
		soloFailure = "Patch could not be validated"
		outputA     = "Source excerpt"
		outputB     = "Fixture matches"
		reasoning   = "Plan the next query"
		answer      = "Inspection complete"
	)
	loads := 0
	turns := make([]Turn, 36)
	for i := range turns[:len(turns)-1] {
		id := fmt.Sprintf("Earlier message %02d", i)
		turns[i] = Turn{ID: id, User: id, InputOnly: true, Controls: Controls{ReadOnly: true}}
	}
	tail := &turns[len(turns)-1]
	*tail = Turn{ID: "tail", User: "Inspect the workspace", Reply: Reply{Status: Completed, Blocks: []Block{
		{Kind: ToolsBlock, Calls: []Call{
			{ID: "inspect", Label: failedRow, Failed: true, Output: &Output{}, Load: func(apply func(*Output)) func() {
				loads++
				apply(&Output{Failure: failure})
				return nil
			}},
			{ID: "read", Label: groupA, Output: &Output{Text: outputA}},
		}},
		{Kind: ReasoningBlock, Text: reasoning},
		{Kind: ToolsBlock, Calls: []Call{
			{ID: "check", Label: warningRow, Warning: true, Output: &Output{Failure: warning, ToolFailure: true}},
			{ID: "search", Label: groupB, Output: &Output{Text: outputB}},
		}},
		{Kind: ToolsBlock, Calls: []Call{{ID: "validate", Label: standalone, Standalone: true, Failed: true, Output: &Output{Failure: soloFailure}}}},
		{Kind: TextBlock, Text: answer},
	}}}
	state, mounted := New(), true
	tester := ui.NewTester(func(c *ui.Context) {
		defer state.FinishFrame()
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() {
			if mounted {
				View(c, state, "session", turns, nil, still{})
			}
		})
	}, 900, 800)
	tester.SetLocale("en")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()

	completed := i18n.In("en", "turn_completed")
	const (
		folded = iota
		summaries
		groupARows
		failedResult
	)
	check := func(stage string, depth, wantLoads int) {
		t.Helper()
		if loads != wantLoads {
			t.Fatalf("%s: loaded tool details %d times, want %d", stage, loads, wantLoads)
		}
		rows := func(label string, expanded ...bool) {
			t.Helper()
			got := rowButtons(tester, label)
			if len(got) != len(expanded) {
				t.Fatalf("%s: %q has %d visible buttons, want %d: %q", stage, label, len(got), len(expanded), tester.Texts())
			}
			for i, want := range expanded {
				if got[i].Expanded != want {
					t.Fatalf("%s: %q button %d expanded = %t, want %t", stage, label, i, got[i].Expanded, want)
				}
			}
		}
		rows(i18n.In("en", StatusKey(tail.Reply.Status)), depth >= summaries)
		if depth == folded {
			rows(groupA)
			rows(groupB)
			rows(standalone)
		} else {
			rows(groupB, false)
			rows(standalone, false)
			if depth >= groupARows {
				rows(groupA, true, false)
				rows(failedRow, depth == failedResult)
			} else {
				rows(groupA, false)
			}
		}
		for _, item := range []struct {
			text string
			show bool
		}{
			{answer, true}, {failedRow, depth >= groupARows}, {failure, depth == failedResult},
			{warningRow, false}, {warning, false}, {soloFailure, false},
			{outputA, false}, {outputB, false}, {reasoning, false},
		} {
			if tester.HasText(item.text) != item.show {
				t.Fatalf("%s: %q visibility should be %t: %q", stage, item.text, item.show, tester.Texts())
			}
		}
	}
	click := func(label string) {
		t.Helper()
		clickRow(t, tester, label)
		tester.Frame()
	}
	remount := func(s *State) {
		mounted = false
		tester.Frame()
		state, mounted = s, true
		tester.Frame()
	}

	check("initial mount", folded, 0)
	click(completed)
	check("opened Completed", summaries, 0)
	click(groupA)
	check("opened group A", groupARows, 0)
	click(failedRow)
	check("opened failed result", failedResult, 1)

	remount(state)
	check("remounted same state", failedResult, 2)

	tail.Reply.Status = Running
	remount(New())
	check("mounted fresh running state", summaries, 2)
	click(groupA)
	check("opened group A while running", groupARows, 2)
	tail.Reply.Status = Completed
	tester.Frame()
	check("finished without selecting the outer fold", folded, 2)
	click(completed)
	check("opened Completed after running", groupARows, 2)
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
