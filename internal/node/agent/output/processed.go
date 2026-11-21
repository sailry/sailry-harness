package output

import (
	"bytes"
	"cmp"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Processed detaches only changed blocks. Pair repair can rename a repeated
// provider call, but read_tool_output needs the ID in the stored result slot.
// Run before Project: a replaced file page must not suppress a later full read.
func Processed(record, original canonical.Record) (canonical.Record, bool) {
	m, stored := record.Message(), original.Message()
	if m == nil || stored == nil || m.Role != canonical.User || record.ID != original.ID {
		return record, false
	}
	var blocks []canonical.Block
	for i, block := range m.Content {
		tool := response(&block)
		if tool == nil || tool.Status != canonical.ToolSuccess || tool.Format != canonical.JSON || len(tool.Result.Value) < 4*1024 {
			continue
		}
		index := slices.IndexFunc(stored.Content, func(b canonical.Block) bool { return b.Index == block.Index })
		if index < 0 {
			continue
		}
		originalBlock := stored.Content[index]
		source := response(&originalBlock)
		if source == nil || source.ID != cmp.Or(tool.OriginalID, tool.ID) || source.Name != tool.Name || source.Revision != tool.Revision ||
			source.Status != tool.Status || source.Format != tool.Format || !bytes.Equal(source.Result.Value, tool.Result.Value) ||
			len(source.Attachments) != 0 || len(source.Result.Assets) != 0 || len(tool.Attachments) != 0 || len(tool.Result.Assets) != 0 {
			continue
		}
		value := processedPage(source)
		if value == nil {
			continue
		}
		if blocks == nil {
			blocks = slices.Clone(m.Content)
		}
		projected := *tool
		projected.Result.Value = value
		blocks[i].Data = &projected
	}
	if blocks == nil {
		return record, false
	}
	copied := *m
	copied.Content = blocks
	record.Data = &copied
	return record, true
}

func processedPage(source *canonical.ToolResponse) json.RawMessage {
	if !validOutputReference(source.ID, source.Revision) {
		return nil
	}
	var summary map[string]any
	switch source.Name {
	case "read_session":
		summary = processedSessionRead(source.Result.Value)
	case "search_session":
		summary = processedSessionSearch(source.Result.Value)
	default:
		summary = processedFilePage(source.Result.Value)
	}
	if summary == nil {
		return nil
	}
	summary["output"] = protocol.ToolOutputOutputReference{Call: source.ID, Revision: source.Revision}
	summary["read"] = "Read the original result with read_tool_output using output and offset=0."
	value := json.RawMessage(encoded(summary))
	if len(value) > 1024 {
		return nil
	}
	return value
}

func processedFilePage(value json.RawMessage) map[string]any {
	var fields map[string]json.RawMessage
	if json.Unmarshal(value, &fields) != nil || len(fields) != 2 {
		return nil
	}
	var summary map[string]any
	switch string(fields["kind"]) {
	case `"file_lines"`:
		var page protocol.FileLines
		if json.Unmarshal(fields["data"], &page) != nil || len(page.Lines) == 0 || page.Path == "" || page.Revision == "" {
			return nil
		}
		summary = map[string]any{"processed": "file_lines", "path": page.Path, "revision": page.Revision, "size": page.Size,
			"first_line": page.Lines[0].Number, "last_line": page.Lines[len(page.Lines)-1].Number,
			"offset": page.Offset, "next_offset": page.NextOffset, "truncated": page.Truncated}
	case `"search_results"`:
		var page protocol.SearchResults
		if json.Unmarshal(fields["data"], &page) != nil || len(page.Diagnostics) > 0 || searchTotal(page) == 0 {
			return nil
		}
		summary = map[string]any{"processed": "search_results", "mode": page.Mode, "rows": searchTotal(page), "scanned_files": page.ScannedFiles,
			"skipped": page.Skipped, "partial": page.Partial, "truncated": page.Truncated, "offset": page.Offset, "next_offset": page.NextOffset}
		switch page.Mode {
		case protocol.SearchModeContent:
			if slices.ContainsFunc(page.Matches, func(m protocol.SearchMatch) bool { return m.Truncated }) {
				return nil
			}
			summary["first_path"], summary["first_line"] = page.Matches[0].Path, page.Matches[0].LineNumber
		case protocol.SearchModeFiles:
			summary["first_path"] = page.Paths[0]
		case protocol.SearchModeCount:
			summary["first_path"], summary["first_count"] = page.Counts[0].Path, page.Counts[0].Count
		}
	default:
		return nil
	}
	return summary
}
