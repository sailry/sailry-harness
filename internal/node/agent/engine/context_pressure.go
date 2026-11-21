package engine

import (
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
)

// A completed main reply records the input usage and window of its own
// request. Its next answered tool batch stays intact. Replaying the first
// qualifying boundary, rather than looking at the latest usage, prevents a
// shorter request from undoing its own projection on the next step.
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
	var positions map[string]int
	start := 0
	for i, record := range history {
		m := record.Message()
		if m == nil || m.Role != canonical.Assistant || m.Finish == nil || *m.Finish != canonical.Completed || m.Error != nil || m.Usage == nil || m.Usage.Input == nil {
			continue
		}
		x := record.Metadata.Execution
		if x == nil || x.Purpose == nil || *x.Purpose != canonical.PurposeMain || x.Observation == nil || x.Observation.ContextLimit == nil {
			continue
		}
		window := uint64(*x.Observation.ContextLimit)
		if window == 0 || *m.Usage.Input < (window+1)/2 {
			continue
		}
		if positions == nil {
			positions = make(map[string]int, len(projected))
			for j := range projected {
				positions[projected[j].ID] = j
			}
		}
		at, visible := positions[record.ID]
		if !visible || !slices.ContainsFunc(projected[at].Message().Content, func(b canonical.Block) bool { return b.Kind == "tool_request" }) {
			continue
		}
		changed := false
		for _, original := range history[start:i] {
			if j, ok := positions[original.ID]; ok && j < at {
				if shortened, ok := output.Processed(projected[j], original); ok {
					projected[j] = shortened
					changed = true
				}
			}
		}
		if changed {
			return
		}
		start = i
	}
}
