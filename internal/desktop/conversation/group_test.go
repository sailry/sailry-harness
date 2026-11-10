package conversation

import (
	"fmt"
	"strings"
	"testing"
	"time"
)

func named(name string, n int) []Call {
	out := make([]Call, n)
	for i := range out {
		out[i] = Call{ID: fmt.Sprintf("%s%d", name, i), Name: name}
	}
	return out
}

// Adjacent tool blocks join, text splits them and blank text does not;
// terminal calls stand alone; adjacent children join.
func TestCompact(t *testing.T) {
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

func TestRunsAndSequence(t *testing.T) {
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
func TestSummary(t *testing.T) {
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
func TestAnswerFrom(t *testing.T) {
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

func TestLastLines(t *testing.T) {
	var lines []string
	for i := range 130 {
		lines = append(lines, fmt.Sprint(i))
	}
	text, hidden := lastLines(strings.Join(lines, "\n") + "\n")
	if hidden != 30 || !strings.HasPrefix(text, "30\n") || !strings.HasSuffix(text, "129") {
		t.Errorf("hidden %d, text starts %q", hidden, text[:4])
	}
}

func TestElapsed(t *testing.T) {
	for d, want := range map[int]string{9: "9s", 65: "1m05s", 3725: "1h02m05s"} {
		if got := Elapsed(secs(d)); got != want {
			t.Errorf("%d s is %q, want %q", d, got, want)
		}
	}
}

func secs(n int) time.Duration { return time.Duration(n) * time.Second }
