package engine

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestContextTokensAfterRollingCompaction(t *testing.T) {
	t.Parallel()
	turn := protocol.NewTurnID()
	old := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Earlier task")), manualNow())
	tail := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Recent evidence")), manualNow())
	stale := uint64(90000)
	tail.Message().Usage = &canonical.Usage{Total: &stale}
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Current handoff"))}, manualNow())
	history := []canonical.Record{old, tail, summary}
	want := tokens.Text("Current handoff") + tokens.Text("Recent evidence") + 14
	if got := contextTokens(history); got != want {
		t.Fatalf("context = %d, want summary plus retained tail %d", got, want)
	}
	measured := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "New answer")), manualNow())
	total := uint64(1234)
	measured.Message().Usage = &canonical.Usage{Total: &total}
	growth := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue")), manualNow())
	history = append(history, measured, growth)
	want = total + tokens.Text("Continue") + 7
	if got := contextTokens(history); got != want {
		t.Fatalf("context = %d, want fresh measurement plus pending input %d", got, want)
	}
}

// A reply whose usage sums several requests counts the window its last
// request occupied, not the sum.
func TestContextTokensOfAReplyThatTookSeveralRequests(t *testing.T) {
	t.Parallel()
	turn := protocol.NewTurnID()
	reply := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer")), manualNow())
	input, output, total, window := uint64(22000), uint64(700), uint64(22700), uint64(12300)
	reply.Message().Usage = &canonical.Usage{Input: &input, Output: &output, Total: &total}
	reply.Metadata.Execution = &canonical.Execution{ContextTokens: &window}
	if got := contextTokens([]canonical.Record{reply}); got != window {
		t.Fatalf("context = %d, want the last request's %d", got, window)
	}
	single := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer")), manualNow())
	single.Message().Usage = &canonical.Usage{Input: &input, Output: &output, Total: &total}
	single.Metadata.Execution = &canonical.Execution{ContextTokens: &total}
	if got := contextTokens([]canonical.Record{single}); got != total {
		t.Fatalf("single request context = %d, want %d", got, total)
	}
}
