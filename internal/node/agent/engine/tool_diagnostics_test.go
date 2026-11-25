package engine

import (
	"context"
	"encoding/json"
	"log/slog"
	"maps"
	"path/filepath"
	"slices"
	"sync"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/hooks"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestToolExecutionDiagnostics(t *testing.T) {
	observed := captureToolDiagnostics(t)
	const reportRef = "64c37d25-b7a2-4162-ba7c-2f6add660791"
	returned := func(origin protocol.ToolFailureOrigin, code *protocol.ErrorCode) ToolResult {
		return ToolResult{Value: "private command output", ReturnedError: true, ReturnedFailure: &protocol.ToolFailure{Origin: origin, Code: code}}
	}
	unknown := returned(protocol.ToolFailureOriginRuntime, new(protocol.ErrorCodeOutcomeUnknown))
	internal := ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "private path: example.dat"}}
	var tools []Tool
	var calls []fantasy.StreamPart
	add := func(id, name string, result ToolResult, operation protocol.ToolOperation) *Tool {
		tool := objectTool(name, true, true, func(context.Context, canonical.ToolRequest) ToolResult { return result })
		if operation != "" {
			tool.Binding = &store.ApprovalBinding{Operation: &operation}
		}
		tools = append(tools, tool)
		calls = append(calls, call(id, name, `{}`))
		return &tools[len(tools)-1]
	}
	streamStarted := make(chan struct{})
	streamed := add("private_stream_id", "private_streamed_tool", internal, "")
	streamed.StreamRead = true
	streamed.Execute = func(context.Context, canonical.ToolRequest) ToolResult {
		close(streamStarted)
		return internal
	}
	add("private_local_id", "private_local_tool", internal, "")
	add("nonzero", "exit_nonzero", returned(protocol.ToolFailureOriginTool, nil), protocol.ToolOperationRunCommand)
	add("foreground", "foreground_unknown", unknown, protocol.ToolOperationRunCommand)
	add("snapshot", "background_snapshot", unknown, protocol.ToolOperationReadCommand)
	add("uncertain", "possible_prior_effect", interruptedTool(true), "")
	add("cancelled", "cancelled_tool", interruptedTool(false), "")
	add("dispatch", "dispatched_tool", ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Dispatch owns this delivery", ReportRef: reportRef, ReportOwned: true}}, "")
	add("plugin", "private_extension_tool", internal, "").Binding = &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "private_extension"}}
	add("preflight", "preflight_tool", internal, "").Prepare = func(context.Context, canonical.ToolRequest) error { return internal.Failure }
	shouldNotExecute := func(context.Context, canonical.ToolRequest) ToolResult {
		t.Error("tool executor ran after its hook failed")
		return internal
	}
	add("hook", "wrapped_tool", internal, "").Execute = shouldNotExecute
	add("panic", "panicking_tool", ToolResult{}, "").Execute = func(context.Context, canonical.ToolRequest) ToolResult { panic("private panic text") }
	calls = append(calls, call("invalid", "private_local_tool", `[]`))

	batch := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		if !yield(calls[0]) {
			return
		}
		select {
		case <-streamStarted:
		case <-ctx.Done():
			return
		}
		response(append(calls[1:], finish())...)(ctx, request, yield)
	}
	f := newFixture(t, &scriptedModel{scripts: []script{batch, response(text("Recovered"), finish())}}, tools...)
	f.r.hooksForTurn = func(context.Context, *store.AgentInvocation) *hooks.Binding {
		settings := protocol.DefaultHooksSettings()
		settings.Enabled = true
		settings.BeforeTool = &protocol.HooksScript{Enabled: true, Tools: []string{"wrapped_tool"}}
		return hooks.New(hooks.Capture{Settings: settings, Admitted: true})
	}
	f.start()
	f.waitRun(f.submit("Run the fixture tools"), protocol.ConversationStatusCompleted)

	internalReport := logging.Fields{"area": "agent", "operation": "tool_execution", "phase": logging.Failed, "code": "internal"}
	foregroundReport := logging.Fields{"area": "agent", "operation": "tool_execution", "phase": logging.Failed, "code": "outcome_unknown", "cause_code": "foreground_command"}
	reports := observed()
	if want := []logging.Fields{internalReport, internalReport, foregroundReport}; !slices.EqualFunc(reports["tool_execution"], want, maps.Equal) {
		t.Fatalf("tool diagnostics = %v, want %v", reports["tool_execution"], want)
	}
	if len(reports["tool"]) != 1 || len(reports["before_tool"]) != 1 || len(reports["turn"]) != 0 {
		t.Fatalf("panic, hook or turn lost its diagnostic ownership: %v", reports)
	}
	responses := map[string]*canonical.ToolResponse{}
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if response, ok := block.Data.(*canonical.ToolResponse); ok {
					responses[response.ID] = response
				}
			}
		}
	}
	for _, call := range calls {
		if result := responses[call.ID]; result == nil || result.Status != canonical.ToolErrorStatus {
			t.Fatalf("call %s did not retain its tool error: %+v", call.ID, result)
		}
	}
	var envelope struct{ Data protocol.Fault }
	if err := json.Unmarshal(responses["dispatch"].Result.Value, &envelope); err != nil || envelope.Data.ReportRef != reportRef {
		t.Fatalf("original fault was changed: %+v, %v", envelope.Data, err)
	}
}

func TestToolExecutionDiagnosticsIgnoreTurnStop(t *testing.T) {
	observed := captureToolDiagnostics(t)
	started := make(chan struct{})
	tool := objectTool("read_until_stopped", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(started)
		<-ctx.Done()
		return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "read released on stop"}}
	})
	f := newFixture(t, &scriptedModel{scripts: []script{response(call("stop", tool.Definition.Name, `{}`), finish())}}, tool)
	f.start()
	turn := f.submit("Start the fixture read")
	select {
	case <-started:
	case <-f.ctx.Done():
		t.Fatal("tool did not start")
	}
	f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
	f.waitRun(turn, protocol.ConversationStatusCancelled)
	if reports := observed(); len(reports) != 0 {
		t.Fatalf("active stop emitted diagnostics: %v", reports)
	}
}

func captureToolDiagnostics(t *testing.T) func() map[string][]logging.Fields {
	t.Helper()
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	var mu sync.Mutex
	reports := map[string][]logging.Fields{}
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		operation, _ := fields["operation"].(string)
		switch operation {
		case "tool_execution", "tool", "before_tool", "turn":
			if level == slog.LevelError {
				mu.Lock()
				reports[operation] = append(reports[operation], fields)
				mu.Unlock()
			}
		}
	})
	t.Cleanup(func() { logging.SetReport(nil) })
	return func() map[string][]logging.Fields {
		mu.Lock()
		defer mu.Unlock()
		return maps.Clone(reports)
	}
}
