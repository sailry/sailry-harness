package engine

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func updatePlanTool(child bool) Tool {
	tool := jsonTool(operations.UpdatePlanTool, "Record the complete current task plan and the state of each step. Submit every step on each update in plan order. Mark steps complete after verification. Multiple steps can be in progress while work proceeds in parallel. This tool records progress in the current conversation.", operations.UpdatePlanSchema(), true, true,
		func(_ context.Context, call canonical.ToolRequest) ToolResult {
			var progress protocol.ConversationProgress
			if err := json.Unmarshal(call.Arguments, &progress); err != nil || !store.ValidAgentProgress(progress) {
				return invalidTool("invalid task progress")
			}
			return ToolResult{Value: map[string]protocol.ConversationProgress{"progress": progress}}
		})
	tool.Deferred = child
	return tool
}
