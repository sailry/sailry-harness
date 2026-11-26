package engine

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func updatePlanTool() Tool {
	return jsonTool(operations.UpdatePlanTool, "Record a full ordered plan snapshot for substantial multi-step work, including unchanged steps; skip for simple or small changes. Always batch with real tool calls in the same turn—never spend a standalone turn solely to create or close out a plan (do not add an extra round just to mark steps completed before answering). Parallel steps may all be in_progress. Mark a step completed only after the work and required checks are verified. A matching successful result confirms that the snapshot was recorded. This tool does not execute or approve work or replace an assistant progress update.", operations.UpdatePlanSchema(), true, true,
		func(_ context.Context, call canonical.ToolRequest) ToolResult {
			var progress protocol.ConversationProgress
			if err := json.Unmarshal(call.Arguments, &progress); err != nil || !store.ValidAgentProgress(progress) {
				return invalidTool("invalid task progress")
			}
			return ToolResult{Value: map[string]protocol.ConversationProgress{"progress": progress}}
		})
}
