package engine

import (
	"context"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A read the same reply already asked for, with equal arguments, is answered
// by reference instead of running again; effects that repeat still run.
func TestRepeatedReadsInOneReplyRunOnce(t *testing.T) {
	t.Parallel()
	var reads, writes atomic.Int32
	read := objectTool("files_read", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		reads.Add(1)
		return ToolResult{Value: map[string]any{}}
	})
	write := objectTool("files_write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
		writes.Add(1)
		return ToolResult{Value: map[string]any{}}
	})
	model := &scriptedModel{scripts: []script{
		response(
			call("a", "files_read", `{"path":"x","limit":5}`),
			call("b", "files_read", `{ "limit": 5, "path": "x" }`),
			call("c", "files_read", `{"path":"y"}`),
			call("d", "files_write", `{"path":"x"}`),
			call("e", "files_write", `{"path":"x"}`),
			finish(),
		),
		response(text("Final answer"), finish()),
	}}
	fixture := newFixture(t, model, read, write)
	fixture.start()
	fixture.waitRun(fixture.submit("Read twice"), protocol.ConversationStatusCompleted)
	if reads.Load() != 2 || writes.Load() != 2 {
		t.Fatalf("reads ran %d times, writes %d; want 2 and 2", reads.Load(), writes.Load())
	}
	answered := false
	for _, record := range fixture.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if value, ok := block.Data.(*canonical.ToolResponse); ok && value.ID == "b" {
					answered = value.Status != canonical.ToolErrorStatus && strings.Contains(string(value.Result.Value), "repeats call a")
				}
			}
		}
	}
	if !answered {
		t.Fatal("the repeated read was not answered by reference")
	}
}
