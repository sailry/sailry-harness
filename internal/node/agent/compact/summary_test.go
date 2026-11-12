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
		{"small window", 32000, 8000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens", ""},
		{"200k window is unchanged", 200000, 64000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens", ""},
		{"400k window", 400000, 64000, 16000, 8000, "under 6,000 tokens", "beyond 8,000 tokens", "4,000"},
		{"1M window", 1000000, 64000, 24000, 12000, "under 9,000 tokens", "beyond 12,000 tokens", "4,000"},
		{"2M window stays at the maximum", 2000000, 64000, 24000, 12000, "under 9,000 tokens", "beyond 12,000 tokens", "4,000"},
		{"model output bounds the cap", 1000000, 10000, 10000, 5000, "under 3,750 tokens", "beyond 5,000 tokens", "12,000"},
		{"small model output keeps the minimum", 1000000, 4096, 4096, 4000, "under 3,000 tokens", "beyond 4,000 tokens", "12,000"},
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
}
