package compact

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

func TestSummaryCapFollowsWindow(t *testing.T) {
	for _, row := range []struct {
		name                     string
		context, modelOutput     uint64
		output, limit            uint64
		target, cut, absentLimit string
	}{
		{"4k window", 4096, 8000, 1024, 512, "under 384 tokens", "beyond 512 tokens", "4,000"},
		{"8k window", 8192, 8000, 2048, 1024, "under 768 tokens", "beyond 1,024 tokens", "4,000"},
		{"8k window with lower model output", 8192, 1024, 1024, 512, "under 384 tokens", "beyond 512 tokens", "4,000"},
		{"32k window is unchanged", 32000, 8000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens", ""},
		{"128k window is unchanged", 128000, 64000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens", ""},
		{"200k window is unchanged", 200000, 64000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens", ""},
		{"400k window", 400000, 64000, 16000, 8000, "under 6,000 tokens", "beyond 8,000 tokens", "4,000"},
		{"1M window", 1000000, 64000, 24000, 12000, "under 9,000 tokens", "beyond 12,000 tokens", "4,000"},
		{"2M window stays at the maximum", 2000000, 64000, 24000, 12000, "under 9,000 tokens", "beyond 12,000 tokens", "4,000"},
		{"model output bounds the cap", 1000000, 10000, 10000, 5000, "under 3,750 tokens", "beyond 5,000 tokens", "12,000"},
		{"small model output bounds the cap", 1000000, 4096, 4096, 2048, "under 1,536 tokens", "beyond 2,048 tokens", "12,000"},
	} {
		t.Run(row.name, func(t *testing.T) {
			output := min(Output(row.context), row.modelOutput)
			if output != row.output || Cap(row.context, output) != row.limit {
				t.Fatalf("output = %d, cap = %d, want %d and %d", output, Cap(row.context, output), row.output, row.limit)
			}
			prompt, err := Prepare(nil, row.context, output)
			if err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(prompt, row.target) || !strings.Contains(prompt, row.cut) || strings.Contains(prompt, "{{") || row.absentLimit != "" && strings.Contains(prompt, row.absentLimit+" tokens") {
				t.Fatalf("prompt limits: %s", prompt[strings.Index(prompt, "Keep the whole handoff"):strings.Index(prompt, "## Output")])
			}
		})
	}
	t.Run("bound", func(t *testing.T) {
		work := strings.Repeat("Preserve state and complete the task. ", 600)
		var concepts []string
		for range 400 {
			concepts = append(concepts, strings.Repeat("Disposable reference ", 10))
		}
		document, _ := json.Marshal(map[string]any{"user_intent": []string{"Fix parser"}, "current_work": work, "technical_concepts": concepts})
		small, err := Parse(string(document), 4000)
		if err != nil {
			t.Fatal(err)
		}
		large, err := Parse(string(document), 12000)
		if err != nil {
			t.Fatal(err)
		}
		smallSize, largeSize := tokens.Text(small.Render()), tokens.Text(large.Render())
		if smallSize > 4000 || largeSize > 12000 || largeSize <= 4000 {
			t.Fatalf("rendered sizes = %d and %d", smallSize, largeSize)
		}
		if len(large.TechnicalConcepts) <= len(small.TechnicalConcepts) || large.CurrentWork == nil || *large.CurrentWork != work {
			t.Fatal("the larger cap kept no more of the handoff")
		}
	})
	t.Run("small cap bounds the rendered handoff", func(t *testing.T) {
		document, _ := json.Marshal(map[string]any{"user_intent": []string{"Fix parser"}, "current_work": strings.Repeat("Preserve state and complete the task. ", 600)})
		limit := Cap(4096, Output(4096))
		summary, err := Parse(string(document), limit)
		if err != nil {
			t.Fatal(err)
		}
		if size := tokens.Text(summary.Render()); size > limit || !strings.Contains(summary.Render(), "Fix parser") {
			t.Fatalf("rendered handoff is %d tokens under cap %d", size, limit)
		}
		if _, err := Parse(`{"user_intent":["Complete the remaining task"]}`, 1); err == nil {
			t.Fatal("accepted a handoff whose essential fields exceed the cap")
		}
	})
}

// Bound must arrive where dropping one entry at a time arrives.
func TestBoundMatchesStepwiseTrim(t *testing.T) {
	build := func() *Summary {
		s := &Summary{UserIntent: []string{"Fix parser"}, PendingTasks: []string{"Add regression tests"}}
		for i := range 40 {
			s.TechnicalConcepts = append(s.TechnicalConcepts, strings.Repeat("concept ", i%7+1))
			s.ErrorsAndFixes = append(s.ErrorsAndFixes, strings.Repeat("fault fixed ", i%5+1))
			s.ProblemSolving = append(s.ProblemSolving, strings.Repeat("step taken ", i%3+1))
		}
		return s
	}
	full := tokens.Text(build().Render())
	for _, limit := range []uint64{full, full - 1, full * 3 / 4, full / 2, full / 3, full / 8, 60, 20} {
		stepwise, bisected := build(), build()
		for tokens.Text(stepwise.Render()) > limit && stepwise.trim() {
		}
		bisected.Bound(limit)
		if stepwise.Render() != bisected.Render() {
			t.Fatalf("limit %d: bisection kept %d/%d/%d entries, stepwise %d/%d/%d", limit,
				len(bisected.TechnicalConcepts), len(bisected.ErrorsAndFixes), len(bisected.ProblemSolving),
				len(stepwise.TechnicalConcepts), len(stepwise.ErrorsAndFixes), len(stepwise.ProblemSolving))
		}
	}
}
