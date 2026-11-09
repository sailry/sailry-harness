package compact

import (
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
)

// Visible projects summary membership and audiences without changing canonical
// records. A summary replaces only history through its captured boundary.
func Visible(records []canonical.Record) []canonical.Record {
	return visible(records, false)
}

// Pending includes empty messages because source token accounting reserves their
// role wrappers even though summary transcripts discard empty content.
func Pending(records []canonical.Record) []canonical.Record {
	return visible(records, true)
}

func visible(records []canonical.Record, keepEmpty bool) []canonical.Record {
	visibility := map[string]bool{}
	boundary, summary := -1, -1
	for i, record := range records {
		if record.Kind == "operation" {
			var operation *canonical.Operation
			switch value := record.Data.(type) {
			case canonical.Operation:
				operation = &value
			case *canonical.Operation:
				operation = value
			}
			if operation != nil && operation.Kind == "visibility" {
				switch value := operation.Data.(type) {
				case canonical.Visibility:
					visibility[value.Message] = value.AgentVisible
				case *canonical.Visibility:
					visibility[value.Message] = value.AgentVisible
				}
			}
		}
		if record.Kind != "compaction" {
			continue
		}
		var compacted *canonical.Compaction
		switch value := record.Data.(type) {
		case canonical.Compaction:
			compacted = &value
		case *canonical.Compaction:
			compacted = value
		}
		if compacted != nil {
			summary = i
			for n := 0; n < i; n++ {
				if records[n].ID == compacted.Boundary {
					boundary, summary = n, i
					break
				}
			}
		}
	}
	var out []canonical.Record
	for i, record := range records {
		visible := record.Metadata.Runtime == nil || record.Metadata.Runtime.AgentVisible
		if override, ok := visibility[record.ID]; ok {
			visible = override
		}
		if i <= boundary || record.Kind == "compaction" && i != summary || !visible {
			continue
		}
		record = output.Audience(record)
		message := record.MutableMessage()
		if message == nil {
			continue
		}
		projected := *message
		projected.Content = nil
		for _, block := range message.Content {
			if record.Metadata.Input != nil && record.Metadata.Input.CapabilityPart != nil && block.Index == *record.Metadata.Input.CapabilityPart {
				continue
			}
			projected.Content = append(projected.Content, block)
		}
		if !keepEmpty && len(projected.Content) == 0 && projected.Error == nil {
			continue
		}
		runtime := canonical.DefaultRuntime()
		if record.Metadata.Runtime != nil {
			runtime = *record.Metadata.Runtime
		}
		runtime.AgentVisible = true
		record.Metadata.Runtime = &runtime
		record.Kind, record.Data = "message", &projected
		out = append(out, record)
	}
	return out
}
