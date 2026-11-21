package output

import (
	"cmp"
	"encoding/json"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
)

func projectCommand(response *canonical.ToolResponse, source map[string]any) bool {
	if response.Format != canonical.JSON || len(response.Result.Assets) != 0 || response.ID == "" || len(response.Revision) != 64 {
		return false
	}
	name := response.Name
	if legacy := operations.LegacyCommandTool(name); legacy != "" {
		name = legacy
	}
	switch name {
	case "run_command", "read_command", "stop_command":
	default:
		return false
	}
	value := object(decode(response.Result.Value))
	if ref := object(value["output"]); ref["call"] == response.ID && ref["revision"] == response.Revision {
		return true
	}
	kind, data := text(value["kind"]), object(value["data"])
	result := map[string]any{"kind": kind, "data": data, "output": source}
	var spans []commandSpan
	switch kind {
	case "command_started":
		if name != "run_command" || !commandInfo(data) {
			return false
		}
	case "command_output", "command_result":
		if kind == "command_output" {
			if name == "run_command" || !commandInfo(object(data["info"])) {
				return false
			}
		} else if name != "run_command" || object(data["outcome"]) == nil {
			return false
		}
		retained := object(value["sailry_command"])
		if retained != nil {
			if retained["id"] == object(data["info"])["id"] {
				delete(retained, "id")
			}
			result["sailry_command"] = retained
		}
		for _, stream := range []string{"stdout", "stderr"} {
			capture := object(data[stream])
			body, ok := capture["text"].(string)
			if !ok {
				return false
			}
			spans = append(spans, commandSpan{fields: capture, key: "text", value: body, recent: true})
			preview := object(retained[stream])
			for _, edge := range []string{"head", "tail"} {
				part, ok := preview[edge].(string)
				if !ok {
					continue
				}
				if edge == "head" && strings.HasPrefix(body, part) || edge == "tail" && strings.HasSuffix(body, part) {
					delete(preview, edge)
				} else {
					spans = append(spans, commandSpan{fields: preview, key: edge, value: part})
				}
			}
		}
		if len(retained) == 0 {
			delete(result, "sailry_command")
		}
	case "command_output_page":
		body, ok := data["text"].(string)
		if name != "read_command" || !ok || !commandInfo(object(data["info"])) {
			return false
		}
		spans = append(spans, commandSpan{fields: data, key: "text", value: body})
	case "commands":
		items := array(value["data"])
		if name != "read_command" || items == nil {
			return false
		}
		for _, item := range items {
			info := object(item)
			command := text(info["command"])
			if !commandInfo(info) {
				return false
			}
			if command != "" {
				first, _, _ := strings.Cut(command, "\n")
				label := first[:headBytes(first, 120)]
				if label != command {
					label += "…"
				}
				info["command_preview"] = label
			}
		}
		result["data"] = items
	case "tool_output":
		if name != "read_command" {
			return false
		}
		page, ok := pagePreview(data)
		if !ok {
			return false
		}
		envelope := map[string]any{"content": []any{map[string]any{"type": "text", "text": page}}}
		bound(envelope, source, false, false)
		response.Format, response.Result.Value = canonical.MCP, json.RawMessage(encoded(envelope))
		return true
	default:
		return false
	}
	if value := commandBody(result, spans); value != nil {
		response.Result.Value = value
		return true
	}
	return false
}

func commandInfo(info map[string]any) bool {
	if text(info["id"]) == "" || object(info["status"]) == nil {
		return false
	}
	for _, key := range []string{"session", "turn", "command", "cwd", "started_ms"} {
		delete(info, key)
	}
	if info["pty"] == false {
		delete(info, "pty")
	}
	if len(array(info["services"])) == 0 {
		delete(info, "services")
	}
	return true
}

type commandSpan struct {
	fields map[string]any
	key    string
	value  string
	recent bool
}

func commandBody(result map[string]any, spans []commandSpan) json.RawMessage {
	if body := encoded(result); len(body) <= MaxBytes {
		return json.RawMessage(body)
	}
	for _, span := range spans {
		span.fields[span.key] = ""
	}
	result["preview_truncated"] = true
	result["read"] = "Read the original result with read_tool_output using output and offset=0. Stream metadata and cursors describe the original result."
	remaining := MaxBytes - len(encoded(result))
	if remaining < 0 {
		return nil
	}
	// Give small streams their full budget first so large stdout cannot hide stderr.
	slices.SortStableFunc(spans, func(a, b commandSpan) int { return cmp.Compare(len(a.value), len(b.value)) })
	for i, span := range spans {
		shown := span.fit(remaining / (len(spans) - i))
		span.fields[span.key] = shown
		remaining -= len(encoded(shown)) - 2
	}
	return json.RawMessage(encoded(result))
}

func (span commandSpan) fit(budget int) string {
	if len(encoded(span.value))-2 <= budget {
		return span.value
	}
	clip := func(n int) string {
		if !span.recent {
			return span.value[:headBytes(span.value, n)]
		}
		start := len(span.value) - n
		for start < len(span.value) && !utf8.RuneStart(span.value[start]) {
			start++
		}
		return span.value[start:]
	}
	lo, hi := 0, min(len(span.value), budget)
	for lo < hi {
		mid := lo + (hi-lo+1)/2
		if len(encoded(clip(mid)))-2 <= budget {
			lo = mid
		} else {
			hi = mid - 1
		}
	}
	return clip(lo)
}
