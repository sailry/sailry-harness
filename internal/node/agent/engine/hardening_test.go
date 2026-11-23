package engine

import (
	"context"
	"encoding/json"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestToolScopedHookPreservesReadConcurrency(t *testing.T) {
	t.Parallel()
	bothRunning := make(chan struct{})
	var active atomic.Int32
	read := objectTool("read_file", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		if active.Add(1) == 2 {
			close(bothRunning)
		}
		select {
		case <-bothRunning:
		case <-time.After(3 * time.Second):
			return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "read calls did not execute concurrently"}}
		case <-ctx.Done():
			return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "cancelled"}}
		}
		return ToolResult{Value: map[string]any{"ok": true}}
	})
	read.StreamRead = true

	var hookHits atomic.Int32
	hookSettings := protocol.DefaultHooksSettings()
	hookSettings.Enabled = true
	hookSettings.BeforeTool = &protocol.HooksScript{
		Command:   "guard-command",
		Enabled:   true,
		TimeoutMs: 5000,
		Tools:     []string{"run_command"},
	}

	model := &scriptedModel{scripts: []script{
		response(call("r1", "read_file", `{}`), call("r2", "read_file", `{}`), finish()),
		response(text("Done"), finish()),
	}}
	fixture := newFixture(t, model, read)
	fixture.r.hooksForTurn = func(_ context.Context, _ *store.AgentInvocation) *hooks.Binding {
		return hooks.New(hooks.Capture{
			Settings: hookSettings,
			Admitted: true,
			Claim:    func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error { return nil },
			Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
				return nil
			},
			Launch: func(context.Context, string, uint64, map[string]string, string) (protocol.ProcessCompletion, error) {
				hookHits.Add(1)
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
			},
		})
	}
	fixture.start()
	fixture.waitRun(fixture.submit("Read concurrently with scoped hook"), protocol.ConversationStatusCompleted)
	if hookHits.Load() != 0 {
		t.Fatalf("scoped run_command hook fired %d times for read_file", hookHits.Load())
	}
}

func TestMainTurnDoomLoopGuard(t *testing.T) {
	t.Parallel()
	failing := objectTool("flaky_edit", false, false, func(_ context.Context, _ canonical.ToolRequest) ToolResult {
		return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "old text was not found in the file"}}
	})
	var scripts []script
	for i := range doomLoopAbortAt + 2 {
		scripts = append(scripts, response(call("c"+string(rune('a'+i)), "flaky_edit", `{"path":"a.go","old":"x"}`), finish()))
	}
	model := &scriptedModel{scripts: scripts}
	fixture := newFixture(t, model, failing)
	fixture.start()
	run := fixture.waitRun(fixture.submit("Try edit"), protocol.ConversationStatusFailed)
	if run.Error == nil || !strings.Contains(run.Error.Message, "consecutive identical failing tool calls") {
		t.Fatalf("expected doom-loop failure, got %+v", run.Error)
	}
	if len(model.calls) != doomLoopAbortAt {
		t.Fatalf("expected exactly %d model steps before doom-loop abort, got %d", doomLoopAbortAt, len(model.calls))
	}
	// Step 4 (index 3) should have seen the warning appended on step 3's tool failure.
	promptAfterWarn := promptText(model.calls[doomLoopWarnAt])
	if !strings.Contains(promptAfterWarn, "Repeated identical failure 3 times") {
		t.Fatalf("expected doom-loop recovery warning after 3rd identical failure, prompt:\n%s", promptAfterWarn)
	}
}

func TestMainTurnDoomLoopResetsOnProgress(t *testing.T) {
	t.Parallel()
	var calls atomic.Int32
	tool := objectTool("edit_step", false, false, func(_ context.Context, req canonical.ToolRequest) ToolResult {
		var args struct {
			OK bool `json:"ok"`
		}
		_ = json.Unmarshal(req.Arguments, &args)
		calls.Add(1)
		if args.OK {
			return ToolResult{Value: map[string]any{"ok": true}}
		}
		return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "bad"}}
	})
	model := &scriptedModel{scripts: []script{
		response(call("c1", "edit_step", `{"ok":false}`), finish()),
		response(call("c2", "edit_step", `{"ok":false}`), finish()),
		response(call("c3", "edit_step", `{"ok":false}`), finish()),
		response(call("c4", "edit_step", `{"ok":true}`), finish()), // resets streak
		response(call("c5", "edit_step", `{"ok":false}`), finish()),
		response(call("c6", "edit_step", `{"ok":false}`), finish()),
		response(call("c7", "edit_step", `{"ok":false}`), finish()),
		response(text("Recovered"), finish()),
	}}
	fixture := newFixture(t, model, tool)
	fixture.start()
	fixture.waitRun(fixture.submit("Recover"), protocol.ConversationStatusCompleted)
	if calls.Load() != 7 {
		t.Fatalf("expected all 7 tool calls to run after reset, got %d", calls.Load())
	}
}

func TestMainTurnStepLimit(t *testing.T) {
	t.Parallel()
	step := objectTool("tick", true, true, func(_ context.Context, _ canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]any{"ok": true}}
	})
	var scripts []script
	for i := range 6 {
		scripts = append(scripts, response(call("t"+string(rune('0'+i)), "tick", `{}`), finish()))
	}
	model := &scriptedModel{scripts: scripts}
	fixture := newFixture(t, model, step)
	fixture.r.maxModelSteps = 3
	fixture.start()
	run := fixture.waitRun(fixture.submit("Loop forever"), protocol.ConversationStatusFailed)
	if run.Error == nil || !strings.Contains(run.Error.Message, "3-step safety limit") {
		t.Fatalf("expected step-limit failure, got %+v", run.Error)
	}
	if len(model.calls) != 3 {
		t.Fatalf("expected exactly 3 model calls, got %d", len(model.calls))
	}
}
