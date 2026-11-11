package conversation

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
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
