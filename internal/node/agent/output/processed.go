package output

import (
	"bytes"
	"cmp"
	"encoding/json"
	"slices"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const processedMaxBytes = 1024

// Processed detaches only changed blocks. Pair repair can rename a repeated
// provider call, but read_tool_output needs the ID in the stored result slot.
// Run before Project: a replaced file page must not suppress a later full read.
func Processed(record, original canonical.Record) (canonical.Record, bool) {
	m, stored := record.Message(), original.Message()
	if m == nil || stored == nil || m.Role != canonical.User || stored.Role != canonical.User || record.ID != original.ID {
		return record, false
	}
	var blocks []canonical.Block
	for i, block := range m.Content {
		tool := response(&block)
		if tool == nil || tool.Status != canonical.ToolSuccess || len(tool.Result.Value) <= processedMaxBytes {
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
		projected.Format = canonical.JSON
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

func protectedProcessedTool(name string) bool {
	switch name {
	case "load_skill", "read_skill_resource", "read_skill_document", "tool_search", "ask_user", "update_plan":
		return true
	}
	return false
}

func processedPage(source *canonical.ToolResponse) json.RawMessage {
	if protectedProcessedTool(source.Name) {
		return nil
	}
	raw := source.Result.Value
	if !validOutputReference(source.ID, source.Revision) || !utf8.ValidString(source.ID) || !utf8.Valid(raw) || !json.Valid(raw) {
		return nil
	}
	var summary map[string]any
	switch source.Format {
	case canonical.JSON:
		switch source.Name {
		case "read_session":
			summary = processedSessionRead(raw)
		case "search_session":
			summary = processedSessionSearch(raw)
		default:
			summary = processedJSON(source)
		}
	case canonical.MCP:
		if commandToolName(source.Name) == "" {
			summary = processedMCP(object(decode(raw)), len(raw))
		}
	}
	if summary == nil {
		return nil
	}
	summary["output"] = protocol.ToolOutputOutputReference{Call: source.ID, Revision: source.Revision}
	summary["read"] = "Read the original result with read_tool_output using output and offset=0."
	body := json.RawMessage(encoded(summary))
	if len(body) > processedMaxBytes {
		return nil
	}
	return body
}

func processedJSON(source *canonical.ToolResponse) map[string]any {
	raw := source.Result.Value
	value := decode(raw)
	fields := object(value)
	kind := text(fields["kind"])
	if kind == "file_lines" || kind == "search_results" {
		return processedFilePage(raw)
	}
	data := object(fields["data"])
	if protectedProcessedFields(fields) || kind != "" && protectedProcessedFields(data) {
		return nil
	}
	if kind == "command_result" {
		return processedCommand(fields, data, len(raw))
	}
	if commandToolName(source.Name) != "" || fields["sailry_command"] != nil || data["sailry_command"] != nil {
		return nil
	}
	if looksLikeProcessedMCP(fields) {
		return processedMCP(fields, len(raw))
	}
	summary := map[string]any{"processed": "json", "bytes": len(raw)}
	switch fields := value.(type) {
	case string:
		summary["processed"] = "text"
	case []any:
		summary["items"] = len(fields)
	case map[string]any:
		_, tagged := fields["kind"]
		_, wrapped := fields["data"]
		if tagged && wrapped {
			var result protocol.Output
			if json.Unmarshal(raw, &result) != nil {
				return nil
			}
			summary["processed"] = result.Kind
			fields = data
		}
		processedFacts(summary, fields)
	default:
		return nil
	}
	return summary
}

func looksLikeProcessedMCP(fields map[string]any) bool {
	if _, ok := fields["content"]; !ok {
		return false
	}
	if _, ok := fields["structuredContent"]; ok {
		return true
	}
	if _, ok := fields["isError"]; ok {
		return true
	}
	return slices.ContainsFunc(array(fields["content"]), func(part any) bool { return object(part)["type"] != nil })
}

func processedMCP(fields map[string]any, size int) map[string]any {
	content, ok := fields["content"]
	parts := array(content)
	structuredValue := fields["structuredContent"]
	structured := object(structuredValue)
	if !ok || content != nil && parts == nil || len(parts) == 0 && len(structured) == 0 ||
		structuredValue != nil && structured == nil {
		return nil
	}
	if protectedProcessedFields(fields) || protectedProcessedFields(structured) || protectedProcessedFields(object(structured["data"])) ||
		fields["sailry_command"] != nil || structured["sailry_command"] != nil || structured["kind"] == "command_result" {
		return nil
	}
	if flag, ok := fields["isError"]; ok && flag != false {
		return nil
	}
	for _, content := range parts {
		part := object(content)
		if restrictedProcessedAudience(part) {
			return nil
		}
		switch part["type"] {
		case "text":
			if _, ok := part["text"].(string); !ok {
				return nil
			}
		case "resource":
			resource := object(part["resource"])
			if _, ok := resource["text"].(string); !ok || resource["blob"] != nil || restrictedProcessedAudience(resource) {
				return nil
			}
		default:
			return nil
		}
	}
	summary := map[string]any{"processed": "mcp", "bytes": size}
	processedFacts(summary, structured)
	return summary
}

func processedFacts(summary, fields map[string]any) {
	for _, key := range []string{"path", "url", "revision", "branch", "status", "size", "additions", "deletions", "offset", "next_offset", "truncated", "partial", "untrusted"} {
		value, present := fields[key]
		switch value := value.(type) {
		case string:
			if len(value) <= 192 && len(encoded(value)) <= 192 {
				summary[key] = value
			}
		case json.Number, bool:
			summary[key] = value
		case nil:
			if key == "next_offset" && present {
				summary[key] = nil
			}
		}
	}
}

func restrictedProcessedAudience(fields map[string]any) bool {
	annotations := fields["annotations"]
	audience, ok := object(annotations)["audience"]
	return ok && (array(audience) == nil || !permitted(annotations))
}

func protectedProcessedFields(fields map[string]any) bool {
	if restrictedProcessedAudience(fields) {
		return true
	}
	switch text(fields["type"]) {
	case "image", "audio":
		return true
	}
	switch text(fields["status"]) {
	case "error", "failed", "cancelled", "timed_out", "timeout", "unknown", "rejected", "partial":
		return true
	}
	switch text(fields["kind"]) {
	case "file_image", "media", "command_started", "command_output", "command_output_page":
		return true
	}
	for _, key := range []string{"isError", "error", "errors", "failure", "diagnostics", "warnings", "issues", "notice", "sandbox_notice"} {
		if hasProcessedEvidence(fields[key]) {
			return true
		}
	}
	return false
}

func hasProcessedEvidence(value any) bool {
	switch value := value.(type) {
	case nil:
		return false
	case bool:
		return value
	case string:
		return value != ""
	case []any:
		return len(value) != 0
	case map[string]any:
		return len(value) != 0
	default:
		return true
	}
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
