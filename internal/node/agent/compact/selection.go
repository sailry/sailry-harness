package compact

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

// Selection replaces Prefix through Boundary and leaves Tail in canonical
// history. Prefix retains the prior compaction record so Prepare can pin it.
type Selection struct {
	Prefix   []canonical.Record
	Tail     []canonical.Record
	Boundary canonical.Record
}

// Select keeps recent complete turns within a soft budget of 20% of context,
// capped at 20,000 tokens. A long turn can split only between complete tool
// exchanges. The newest exchange stays whole even when it exceeds the budget.
// keepRecent=false selects all visible history; the caller preserves any
// unprocessed user input when using that fallback.
func Select(records []canonical.Record, contextLimit uint64, keepRecent bool) (Selection, bool) {
	visible := Visible(records)
	if len(visible) == 0 {
		return Selection{}, false
	}
	var previous *canonical.Record
	for i := range records {
		if records[i].Kind == "compaction" {
			previous = &records[i]
		}
	}
	start := 0
	if previous != nil && visible[0].ID == previous.ID {
		start = 1
	}
	if start == len(visible) {
		return Selection{}, false
	}
	cut := len(visible)
	if keepRecent {
		budget := min(uint64(20000), contextLimit/5)
		var cache output.Cache
		projected := cache.Project(visible)
		suffix := make([]uint64, len(visible)+1)
		for i := len(visible) - 1; i >= start; i-- {
			size := tokens.Chat("", projected[i:i+1])
			for _, block := range visible[i].Message().Content {
				switch value := block.Data.(type) {
				case *canonical.Thinking:
					size += tokens.Text(value.Text)
				case *canonical.RedactedThinking:
					for _, text := range value.Data {
						size += tokens.Text(text)
					}
				case *canonical.AssetContent, *canonical.FileContent:
					// Materialization belongs to the next main request. Unknown
					// media conservatively occupies the tail budget without IO.
					size += max(budget, 1)
				case *canonical.ToolResponse:
					size += toolMediaCount(value) * max(budget, 1)
				}
			}
			suffix[i] = suffix[i+1] + size
		}
		cuts := []int{start}
		pending := map[string]bool{}
		for i := start; i < len(visible); i++ {
			for _, block := range visible[i].Message().Content {
				switch value := block.Data.(type) {
				case *canonical.ToolRequest:
					if value.Error == nil {
						pending[value.ID] = true
					}
				case *canonical.ToolResponse:
					delete(pending, value.ID)
				}
			}
			if len(pending) == 0 && !turnContext(visible[i]) {
				cuts = append(cuts, i+1)
			}
		}
		if cuts[len(cuts)-1] != len(visible) {
			cuts = append(cuts, len(visible))
		}
		cut = -1
		for _, candidate := range cuts[:len(cuts)-1] {
			if !turnContext(visible[candidate]) && kickoff(visible[candidate].Message()) && suffix[candidate] <= budget {
				cut = candidate
				break
			}
		}
		if cut < 0 {
			cut = cuts[len(cuts)-2]
			for _, candidate := range cuts[:len(cuts)-1] {
				if suffix[candidate] <= budget {
					cut = candidate
					break
				}
			}
		}
	}
	worthwhile := false
	for _, record := range visible[start:cut] {
		message := record.Message()
		if turnContext(record) || record.Metadata.Runtime != nil && !record.Metadata.Runtime.UserVisible && !hasTools(message) {
			continue
		}
		for _, block := range message.Content {
			switch value := block.Data.(type) {
			case *canonical.TextContent:
				worthwhile = worthwhile || strings.TrimSpace(value.Text) != ""
			case *canonical.AssetContent, *canonical.FileContent, *canonical.ToolRequest, *canonical.ToolResponse:
				worthwhile = true
			}
		}
	}
	if !worthwhile {
		return Selection{}, false
	}
	selected := Selection{Prefix: slices.Clone(visible[:cut]), Tail: slices.Clone(visible[cut:])}
	if start == 1 {
		var summary canonical.Compaction
		switch value := previous.Data.(type) {
		case *canonical.Compaction:
			summary = *value
		case canonical.Compaction:
			summary = value
		}
		summary.Summary = *selected.Prefix[0].Message()
		selected.Prefix[0].Kind, selected.Prefix[0].Data = "compaction", &summary
	}
	for _, record := range records {
		if record.ID == visible[cut-1].ID {
			selected.Boundary = record
			break
		}
	}
	return selected, true
}

func turnContext(record canonical.Record) bool {
	return record.Metadata.Runtime != nil && record.Metadata.Runtime.TurnContext
}

func toolMediaCount(response *canonical.ToolResponse) uint64 {
	count := uint64(len(response.Result.Assets) + len(response.Attachments))
	if response.Format != canonical.MCP {
		return count
	}
	var envelope struct {
		Content []struct {
			Type     string `json:"type"`
			Resource struct {
				Blob json.RawMessage `json:"blob"`
			} `json:"resource"`
		} `json:"content"`
	}
	if json.Unmarshal(response.Result.Value, &envelope) == nil {
		for _, part := range envelope.Content {
			switch part.Type {
			case "image", "audio":
				count++
			case "resource":
				if len(part.Resource.Blob) > 0 {
					count++
				}
			}
		}
	}
	return count
}
