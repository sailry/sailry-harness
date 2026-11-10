package engine

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Anonymous usage counters: results, modes, tool and model names from the
// reviewed lists, error codes, durations and token counts. Never content.

// observeTurn counts a finished task turn once, after its run committed.
func (r *Runtime) observeTurn(run protocol.ConversationRun, mode protocol.WorkMode, subagent bool) {
	if run.Kind != protocol.ConversationRunKindTask {
		return
	}
	var result string
	switch run.Status {
	case protocol.ConversationStatusCompleted, protocol.ConversationStatusContinued:
		result = "completed"
	case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted:
		result = "failed"
	case protocol.ConversationStatusCancelled:
		result = "cancelled"
	default:
		return
	}
	var code *protocol.ErrorCode
	if run.Error != nil {
		code = &run.Error.Code
	} else if result == "cancelled" {
		cancelled := protocol.ErrorCodeCancelled
		code = &cancelled
	}
	r.telemetry.Observe(telemetry.Turn(result, mode, subagent, code))
}

// observeTool counts a tool call under the consent it started with.
// Workspace tools outside the reviewed names are not counted.
func observeTool(metric *telemetry.Capture, ctx context.Context, name string, result ToolResult, started time.Time) {
	tool, counted := telemetry.CaptureTool(name)
	if metric == nil || !counted {
		return
	}
	outcome, code := "success", (*protocol.ErrorCode)(nil)
	switch {
	case ctx.Err() != nil:
		cancelled := protocol.ErrorCodeCancelled
		outcome, code = "cancelled", &cancelled
	case result.Failure != nil && result.Failure.Code == protocol.ErrorCodePermissionDenied:
		outcome, code = "denied", &result.Failure.Code
	case result.Failure != nil:
		outcome, code = "error", &result.Failure.Code
	case result.Error || result.ReturnedError:
		outcome = "error"
	}
	metric.Record(telemetry.ToolCall(tool, outcome, code, time.Since(started)))
}

// observeModel counts one model request.
func (r *Runtime) observeModel(metric *telemetry.Capture, binding model.Binding, attempt model.Attempt) {
	if metric == nil {
		return
	}
	outcome, code := "success", (*protocol.ErrorCode)(nil)
	if failure := attempt.Failure; failure != nil {
		outcome = "error"
		fault := model.Fault(failure).Code
		if failure.Kind == "cancelled" {
			outcome, fault = "cancelled", protocol.ErrorCodeCancelled
		}
		code = &fault
	}
	count := func(value *uint64) uint64 {
		if value == nil {
			return 0
		}
		return *value
	}
	usage := attempt.Usage
	tokens := telemetry.Tokens{Input: count(usage.Input), Output: count(usage.Output), CacheRead: count(usage.CacheRead), CacheWrite: count(usage.CacheWrite)}
	metric.Record(telemetry.ModelRequest(r.telemetry.Model(binding.Provider, binding.Config.Model), outcome, attempt.Status, code, nil, tokens))
}
