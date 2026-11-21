package output

import (
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type processedHistoryRun struct {
	Turn  protocol.TurnID `json:"turn"`
	Error json.RawMessage `json:"error"`
}

func (run processedHistoryRun) failed() bool {
	return len(run.Error) != 0 && string(run.Error) != "null"
}

type processedHistoryEntry struct {
	Parts []struct {
		Kind string          `json:"kind"`
		Data json.RawMessage `json:"data"`
	} `json:"parts"`
}

func (entry processedHistoryEntry) diagnostic() bool {
	for _, part := range entry.Parts {
		if part.Kind != "resource" {
			continue
		}
		var data struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(part.Data, &data) != nil {
			continue
		}
		switch data.Type {
		case "tool_failed", "compaction_failure", "model_retry", "output_limit", "turn_limit":
			return true
		}
	}
	return false
}

func processedSessionRead(value json.RawMessage) map[string]any {
	var result struct {
		Page *struct {
			Session    protocol.SessionID      `json:"session"`
			Revision   *uint64                 `json:"revision"`
			NextBefore *protocol.TurnID        `json:"next_before"`
			Entries    []processedHistoryEntry `json:"entries"`
			Runs       []processedHistoryRun   `json:"runs"`
		} `json:"page"`
		Run              *processedHistoryRun    `json:"run"`
		Revision         *uint64                 `json:"revision"`
		NextBefore       *uint64                 `json:"next_before"`
		Entries          []processedHistoryEntry `json:"entries"`
		Missing          []protocol.TurnID       `json:"missing"`
		Truncated        bool                    `json:"truncated"`
		TruncatedEntries []string                `json:"truncated_entries"`
		TruncatedInputs  []protocol.TurnID       `json:"truncated_inputs"`
	}
	if json.Unmarshal(value, &result) != nil {
		return nil
	}
	summary := map[string]any{"processed": "read_session", "truncated": result.Truncated,
		"truncated_entries": result.TruncatedEntries, "truncated_inputs": result.TruncatedInputs}
	switch {
	case result.Page != nil && result.Run == nil:
		page := result.Page
		if page.Session == (protocol.SessionID{}) || page.Revision == nil || len(page.Entries)+len(page.Runs) == 0 ||
			slices.ContainsFunc(page.Runs, processedHistoryRun.failed) || slices.ContainsFunc(page.Entries, processedHistoryEntry.diagnostic) {
			return nil
		}
		summary["page"] = map[string]any{"session": page.Session, "revision": *page.Revision, "next_before": page.NextBefore,
			"entry_count": len(page.Entries), "run_count": len(page.Runs)}
		summary["missing"] = result.Missing
	case result.Run != nil && result.Page == nil:
		if result.Run.Turn == (protocol.TurnID{}) || result.Revision == nil || result.Run.failed() ||
			slices.ContainsFunc(result.Entries, processedHistoryEntry.diagnostic) {
			return nil
		}
		summary["run"] = map[string]any{"turn": result.Run.Turn}
		summary["revision"], summary["next_before"], summary["entry_count"] = *result.Revision, result.NextBefore, len(result.Entries)
	default:
		return nil
	}
	return summary
}

func processedSessionSearch(value json.RawMessage) map[string]any {
	var page protocol.ConversationSearchPage
	if json.Unmarshal(value, &page) != nil || page.Session == (protocol.SessionID{}) || len(page.Matches) == 0 {
		return nil
	}
	first := page.Matches[0]
	return map[string]any{"processed": "search_session", "session": page.Session, "revision": page.Revision,
		"next_before": page.NextBefore, "match_count": len(page.Matches), "first_turn": first.Turn, "first_entry": first.Entry, "first_sequence": first.Sequence}
}
