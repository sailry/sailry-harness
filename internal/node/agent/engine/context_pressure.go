package engine

import (
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
)

const toolOutputPressureBytes = 96 * 1024

// Replay pressure at answered batch boundaries to keep already shortened
// results stable as history grows. Each boundary protects its newest batch.
func projectContextPressure(projected, history []canonical.Record, tools []fantasy.FunctionTool) {
	if !slices.ContainsFunc(tools, func(t fantasy.FunctionTool) bool { return t.Name == "read_tool_output" }) {
		return
	}
	for i := len(history) - 1; i >= 0; i-- {
		if history[i].Kind == "compaction" {
			history = history[i+1:]
			break
		}
	}
	positions := make(map[string]int, len(projected))
	for i := range projected {
		positions[projected[i].ID] = i
	}

	var start, batchStart, olderBytes, batchBytes int
	var windowPressure, usedWindowPressure bool
	finishBatch := func() {
		if batchBytes == 0 {
			return
		}
		if olderBytes > 0 && (olderBytes+batchBytes >= toolOutputPressureBytes || windowPressure && !usedWindowPressure) {
			for _, original := range history[start:batchStart] {
				if j, ok := positions[original.ID]; ok {
					if shortened, ok := output.Processed(projected[j], original); ok {
						projected[j] = shortened
						usedWindowPressure = usedWindowPressure || windowPressure
					}
				}
			}
			start, olderBytes = batchStart, 0
		}
		olderBytes += batchBytes
		batchBytes = 0
	}
	for i, original := range history {
		j, visible := positions[original.ID]
		if !visible {
			continue
		}
		m := projected[j].Message()
		if m == nil {
			continue
		}
		if m.Role == canonical.Assistant && slices.ContainsFunc(m.Content, func(b canonical.Block) bool { return b.Kind == "tool_request" }) {
			finishBatch()
			batchStart, windowPressure = i, mainReplyOverHalfWindow(original, m)
		} else if m.Role == canonical.User {
			for _, block := range m.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					batchBytes += min(len(result.Result.Value), output.MaxBytes)
				}
			}
		}
	}
	finishBatch()
}

func mainReplyOverHalfWindow(record canonical.Record, m *canonical.Message) bool {
	if m.Finish == nil || *m.Finish != canonical.Completed || m.Error != nil || m.Usage == nil || m.Usage.Input == nil {
		return false
	}
	x := record.Metadata.Execution
	if x == nil || x.Purpose == nil || *x.Purpose != canonical.PurposeMain || x.Observation == nil || x.Observation.ContextLimit == nil {
		return false
	}
	window := uint64(*x.Observation.ContextLimit)
	return window != 0 && *m.Usage.Input >= (window+1)/2
}
