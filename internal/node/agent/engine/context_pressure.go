package engine

import (
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
)

const (
	toolOutputPressureBytes      = 96 * 1024
	mediumWindowPressureBytes    = 256 * 1024
	largeWindowPressureBytes     = 512 * 1024
	hugeWindowPressureBytes      = 1536 * 1024
	smallContextWindowThreshold  = 64_000
	mediumContextWindowThreshold = 256_000
	largeContextWindowThreshold  = 512_000
)

func mainContextWindow(record canonical.Record) uint64 {
	x := record.Metadata.Execution
	if x == nil || x.Purpose == nil || *x.Purpose != canonical.PurposeMain || x.Observation == nil || x.Observation.ContextLimit == nil {
		return 0
	}
	return uint64(*x.Observation.ContextLimit)
}

func pressureLimitBytes(window uint64) int {
	switch {
	case window > largeContextWindowThreshold:
		return hugeWindowPressureBytes
	case window > mediumContextWindowThreshold:
		return largeWindowPressureBytes
	case window > smallContextWindowThreshold:
		return mediumWindowPressureBytes
	default:
		return toolOutputPressureBytes
	}
}

// Replay pressure at answered batch boundaries to keep already shortened
// results stable as history grows. Each boundary protects its newest batch.
func projectContextPressure(projected, history []canonical.Record, tools []fantasy.FunctionTool) {
	if !slices.ContainsFunc(tools, func(t fantasy.FunctionTool) bool { return t.Name == "read_tool_output" }) {
		return
	}
	checkpoint := -1
	for i := len(history) - 1; i >= 0; i-- {
		if history[i].Kind == "compaction" {
			checkpoint = i
			break
		}
	}
	positions := make(map[string]int, len(projected))
	for i := range projected {
		positions[projected[i].ID] = i
	}
	var window uint64
	pressureBytes := toolOutputPressureBytes

	var start, batchStart, olderBytes, batchBytes int
	var windowPressure, usedWindowPressure bool
	finishBatch := func() {
		if batchBytes == 0 {
			return
		}
		if batchStart > checkpoint && olderBytes > 0 && (olderBytes+batchBytes >= pressureBytes || windowPressure && !usedWindowPressure) {
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
		if original.Kind == "compaction" {
			window = 0
		}
		j, visible := positions[original.ID]
		if !visible {
			continue
		}
		m := projected[j].Message()
		if m == nil {
			continue
		}
		if observed := mainContextWindow(original); observed != 0 {
			window = observed
		}
		if m.Role == canonical.Assistant && slices.ContainsFunc(m.Content, func(b canonical.Block) bool { return b.Kind == "tool_request" }) {
			finishBatch()
			pressureBytes = pressureLimitBytes(window)
			// A new turn can fold finished work earlier while a large single
			// turn keeps its working set and its cached request prefix.
			if olderBytes > 0 && original.Turn != history[batchStart].Turn {
				pressureBytes = min(pressureBytes, mediumWindowPressureBytes)
			}
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
	window := mainContextWindow(record)
	if window == 0 {
		return false
	}
	if window > mediumContextWindowThreshold {
		return *m.Usage.Input*100 >= window*65
	}
	if window > smallContextWindowThreshold {
		return *m.Usage.Input*100 >= window*60
	}
	return *m.Usage.Input >= (window+1)/2
}
