package store

import (
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var agentPlanTool = operations.Alias("progress", "update_plan")

// LatestAgentPlan returns the last successful plan in chronological session
// history unless all its steps are completed or skipped.
func LatestAgentPlan(records []canonical.Record) *protocol.ConversationProgress {
	type callKey struct {
		turn       protocol.TurnID
		branch, id string
	}
	type receipt struct {
		record int
		block  canonical.Block
	}
	var pending map[callKey]receipt
	var latest *protocol.ConversationProgress
	var latestReceipt receipt
	for i := len(records) - 1; i >= 0; i-- {
		record := records[i]
		if record.Provenance.Author != "tool" && (record.Provenance.Author != "assistant" || len(pending) == 0) {
			continue
		}
		message := record.Message()
		if message == nil {
			continue
		}
		switch {
		case record.Provenance.Author == "tool" && message.Role == canonical.User:
			for j := len(message.Content) - 1; j >= 0; j-- {
				block := message.Content[j]
				response, ok := block.Data.(*canonical.ToolResponse)
				if !ok || response == nil || response.Name != agentPlanTool || response.ID == "" {
					continue
				}
				key := callKey{record.Turn, record.Provenance.Branch, response.ID}
				// An earlier receipt consumes this request before a duplicate can.
				if latest != nil || response.Status != canonical.ToolSuccess || response.Format != canonical.JSON {
					delete(pending, key)
					continue
				}
				if pending == nil {
					pending = make(map[callKey]receipt)
				}
				pending[key] = receipt{i, block}
			}
		case record.Provenance.Author == "assistant" && message.Role == canonical.Assistant:
			for _, block := range message.Content {
				request, ok := block.Data.(*canonical.ToolRequest)
				if !ok || request == nil || request.Name != agentPlanTool {
					continue
				}
				key := callKey{record.Turn, record.Provenance.Branch, request.ID}
				result, ok := pending[key]
				if !ok {
					continue
				}
				delete(pending, key)
				if request.Error != nil || latest != nil && (result.record < latestReceipt.record ||
					result.record == latestReceipt.record && result.block.Index <= latestReceipt.block.Index) {
					continue
				}
				raw := rawTool(records[result.record], result.block)
				raw.Arguments = request.Arguments
				raw.Presentation = protocol.ToolPresentationProgress
				if plan := raw.progress(); plan != nil {
					latest, latestReceipt = plan, result
				}
			}
		}
		if latest != nil && len(pending) == 0 {
			break
		}
	}
	if latest != nil {
		for _, step := range latest.Steps {
			if step.State == protocol.ConversationProgressStatePending || step.State == protocol.ConversationProgressStateInProgress {
				return latest
			}
		}
	}
	return nil
}
