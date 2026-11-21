package output

import (
	"cmp"
	"encoding/hex"
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
)

func commandToolName(name string) string {
	if legacy := operations.LegacyCommandTool(name); legacy != "" {
		name = legacy
	}
	switch name {
	case "run_command", "read_command", "stop_command", "write_command", "read_tool_output":
		return name
	default:
		return ""
	}
}

func validOutputReference(call, revision string) bool {
	if call == "" || len(call) > 4096 || len(revision) != 64 {
		return false
	}
	_, err := hex.DecodeString(revision)
	return err == nil
}

func projectCommand(response *canonical.ToolResponse, name string, source map[string]any) bool {
	if response.Format != canonical.JSON || len(response.Result.Assets) != 0 || len(response.Attachments) != 0 {
		return false
	}
	value := object(decode(response.Result.Value))
	if name == "read_tool_output" {
		return projectOutputPage(response, value)
	}
	if !validOutputReference(text(source["call"]), text(source["revision"])) {
		return false
	}
	if ref := object(value["output"]); ref["call"] == source["call"] && ref["revision"] == source["revision"] &&
		commandLines(value) <= MaxLines && len(commandJSON(value)) <= MaxBytes {
		return true
	}
	kind, data := text(value["kind"]), object(value["data"])
	result := map[string]any{"kind": kind, "data": data, "output": source}
	var info map[string]any
	var spans []commandSpan
	var captures []commandCapture
	switch kind {
	case "command_started":
		info = data
		if name != "run_command" || !commandInfo(info) {
			return false
		}
	case "command_output", "command_result":
		if kind == "command_output" {
			info = object(data["info"])
			if name == "run_command" || !commandInfo(info) {
				return false
			}
		} else {
			outcome := object(data["outcome"])
			if name != "run_command" || outcome == nil {
				return false
			}
			spans = appendCommandState(spans, outcome)
		}
		retained := object(value["sailry_command"])
		if id := text(retained["id"]); id != "" && id == text(info["id"]) {
			delete(retained, "id")
		}
		for _, stream := range []string{"stdout", "stderr"} {
			capture := object(data[stream])
			body, ok := capture["text"].(string)
			if !ok {
				return false
			}
			recent := kind == "command_output"
			spans = appendCommandSpan(spans, capture, "text", recent)
			if preview := object(retained[stream]); preview != nil {
				captures = append(captures, commandCapture{
					capture: capture, preview: preview, recent: recent,
					captureBytes: len(body), headBytes: len(text(preview["head"])), tailBytes: len(text(preview["tail"])),
				})
				spans = appendCommandSpan(spans, preview, "head", false)
				spans = appendCommandSpan(spans, preview, "tail", true)
			}
		}
		if len(retained) != 0 {
			result["sailry_command"] = retained
		}
	case "command_output_page":
		info = object(data["info"])
		if _, ok := data["text"].(string); name != "read_command" || !ok || !commandInfo(info) {
			return false
		}
		spans = appendCommandSpan(spans, data, "text", false)
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
				first = strings.TrimSuffix(first, "\r")
				label := first[:headBytes(first, 120)]
				if label != command {
					label += "…"
				}
				info["command_preview"] = label
			}
		}
		result["data"] = items
	case "tool_output":
		return name == "read_command" && projectOutputPage(response, data)
	default:
		return false
	}
	spans = appendCommandState(spans, object(info["status"]))
	spans = appendCommandSpan(spans, data, "notice", false)
	spans = appendCommandSpan(spans, data, "sandbox_notice", false)
	if !fitCommandBody(result, info, spans) {
		return false
	}
	for _, capture := range captures {
		capture.deduplicate()
	}
	response.Result.Value = commandJSON(result)
	return true
}

func projectOutputPage(response *canonical.ToolResponse, value any) bool {
	page, ok := pagePreview(value)
	if ok {
		response.Format, response.Result.Value = canonical.MCP, commandJSON(textEnvelope(page))
	}
	return ok
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

type commandCapture struct {
	capture, preview                   map[string]any
	captureBytes, headBytes, tailBytes int
	recent                             bool
}

func (c commandCapture) deduplicate() {
	for _, edge := range []string{"head", "tail"} {
		if c.preview[edge] == "" {
			delete(c.preview, edge)
		}
	}
	// A truncated snapshot has no capture range, and its retained preview can
	// come from a later snapshot. Identical bytes alone do not identify a range.
	if (c.recent && c.capture["truncated"] != false) || c.capture["invalid_utf8"] != false || c.preview["transcoded"] != false {
		return
	}
	body := text(c.capture["text"])
	start := uint64(0)
	if c.recent {
		start = uint64(c.captureBytes - len(body))
	}
	contains := func(offset uint64, part string) bool {
		return part != "" && offset >= start && offset-start <= uint64(len(body)) && strings.HasPrefix(body[offset-start:], part)
	}
	if size, ok := commandUint(c.preview["head_bytes"]); ok && size == uint64(c.headBytes) && c.preview["head_invalid_utf8"] == false && contains(0, text(c.preview["head"])) {
		delete(c.preview, "head")
	}
	tail := text(c.preview["tail"])
	if at, ok := commandUint(c.preview["tail_offset"]); ok && c.preview["invalid_utf8"] == false && tail != "" {
		skipped := uint64(c.tailBytes - len(tail))
		if at <= ^uint64(0)-skipped && contains(at+skipped, tail) {
			delete(c.preview, "tail")
		}
	}
}

func commandUint(value any) (uint64, bool) {
	number, _ := value.(json.Number)
	n, err := strconv.ParseUint(string(number), 10, 64)
	return n, err == nil
}

type commandSpan struct {
	fields map[string]any
	key    string
	value  string
	recent bool
}

func appendCommandSpan(spans []commandSpan, fields map[string]any, key string, recent bool) []commandSpan {
	if value := text(fields[key]); value != "" {
		spans = append(spans, commandSpan{fields: fields, key: key, value: value, recent: recent})
	}
	return spans
}

func appendCommandState(spans []commandSpan, status map[string]any) []commandSpan {
	if outcome := object(status["data"]); outcome != nil {
		status = outcome
	}
	return appendCommandSpan(spans, status, "data", false)
}

// Match the HTML escaping applied when the model builder marshals a tool response.
func commandJSON(value any) json.RawMessage {
	raw, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return raw
}

func commandLines(value any) int {
	lines := 0
	switch value := value.(type) {
	case string:
		return lineCount(value)
	case map[string]any:
		for _, field := range value {
			lines += commandLines(field)
		}
	case []any:
		for _, item := range value {
			lines += commandLines(item)
		}
	}
	return lines
}

func fitCommandBody(result, info map[string]any, spans []commandSpan) bool {
	if commandLines(result) <= MaxLines && len(commandJSON(result)) <= MaxBytes {
		return true
	}
	result["preview_truncated"] = true
	result["read"] = "Read the original result with read_tool_output using output and offset=0. Stream ranges, flags and cursors describe the original result."
	if items := array(result["data"]); items != nil {
		return fitCommandList(result, items)
	}
	return fitCommandInfo(result, info, spans, MaxBytes, MaxLines)
}

func fitCommandInfo(result, info map[string]any, spans []commandSpan, bytes, lines int) bool {
	if fitCommandSpans(result, spans, bytes, lines) {
		return true
	}
	if _, ok := info["services"]; !ok {
		return false
	}
	delete(info, "services")
	return fitCommandSpans(result, spans, bytes, lines)
}

func fitCommandList(result map[string]any, items []any) bool {
	result["data"] = items[:0]
	remainingBytes, remainingLines := MaxBytes-len(commandJSON(result)), MaxLines-commandLines(result)
	if remainingBytes < 0 || remainingLines < 0 {
		return false
	}
	for i, item := range items {
		if i != 0 {
			remainingBytes-- // comma between entries
		}
		bytes, lines := len(commandJSON(item)), commandLines(item)
		if bytes > remainingBytes || lines > remainingLines {
			info := object(item)
			spans := appendCommandState(nil, object(info["status"]))
			if !fitCommandInfo(info, info, spans, remainingBytes, remainingLines) {
				break
			}
			bytes, lines = len(commandJSON(info)), commandLines(info)
		}
		result["data"] = items[:i+1]
		remainingBytes -= bytes
		remainingLines -= lines
	}
	return true
}

const commandErrorPrefix = "Tool error: "

func projectUnshapedCommand(response *canonical.ToolResponse, source map[string]any) bool {
	if len(response.Attachments) != 0 || len(response.Result.Assets) != 0 {
		return false
	}
	value := decode(response.Result.Value)
	if value == nil {
		return false
	}
	failed := response.Status == canonical.ToolErrorStatus || response.Format == canonical.MCPError
	budget := MaxBytes
	if failed {
		budget -= len(commandErrorPrefix)
	}
	lines := 0
	if response.Format == canonical.MCP {
		fields := object(value)
		_, lines = size(fields)
		if structured, ok := fields["structuredContent"]; ok {
			lines += commandLines(structured) - lineCount(encoded(structured))
		}
	} else {
		lines = commandLines(value)
	}
	if lines <= MaxLines && len(commandJSON(value)) <= budget {
		return true
	}
	message := ""
	if failed {
		message = toolFailureText(response.Result.Value)
	}
	projectCommandText(response, source, message, failed)
	return true
}

func projectCommandText(response *canonical.ToolResponse, source map[string]any, message string, failed bool) {
	budget := MaxBytes
	if failed {
		// The model builder can add this prefix after extracting the diagnostic.
		budget -= len(commandErrorPrefix)
	}
	if response.Format != canonical.MCPError {
		response.Format = canonical.MCP
	}
	envelope := func(body string) map[string]any {
		if response.Format == canonical.MCPError {
			return map[string]any{"message": body}
		}
		value := textEnvelope(body)
		if failed {
			value["isError"] = true
		}
		return value
	}
	if message != "" && lineCount(message) <= MaxLines {
		if body := commandJSON(envelope(message)); len(body) <= budget {
			response.Result.Value = body
			return
		}
	}
	footer := "[Tool result truncated.]"
	if validOutputReference(text(source["call"]), text(source["revision"])) {
		footer = "[Tool result truncated. Read the original result with read_tool_output using output=" + encoded(source) + " and offset=0.]"
	}
	overhead := len(commandJSON(envelope("\n" + footer)))
	if shown := fitEncodedText(message, budget-overhead, MaxLines-1, false); shown != "" {
		if !strings.HasSuffix(shown, "\n") {
			shown += "\n"
		}
		footer = shown + footer
	}
	response.Result.Value = commandJSON(envelope(footer))
}

func fitCommandSpans(result map[string]any, spans []commandSpan, bytes, lines int) bool {
	for _, span := range spans {
		span.fields[span.key] = ""
	}
	bytes -= len(commandJSON(result))
	lines -= commandLines(result)
	if bytes < 0 || lines < 0 {
		return false
	}
	// Give small streams their full budget first so large stdout cannot hide stderr.
	slices.SortStableFunc(spans, func(a, b commandSpan) int { return cmp.Compare(len(a.value), len(b.value)) })
	for i, span := range spans {
		left := len(spans) - i
		shown := fitEncodedText(span.value, bytes/left, lines/left, span.recent)
		span.fields[span.key] = shown
		bytes -= len(commandJSON(shown)) - 2
		lines -= lineCount(shown)
	}
	return true
}

func fitEncodedText(value string, bytes, lines int, recent bool) string {
	if bytes <= 0 || lines <= 0 {
		return ""
	}
	if recent {
		value = value[tailStart(value, len(value), lines):]
	} else {
		value = value[:head(value, len(value), lines)]
	}
	if len(value) <= bytes && len(commandJSON(value))-2 <= bytes {
		return value
	}
	clip := func(n int) string {
		if !recent {
			end := headBytes(value, n)
			if end > 0 && end < len(value) && value[end-1] == '\r' && value[end] == '\n' {
				end--
			}
			return value[:end]
		}
		start := len(value) - n
		for start < len(value) && !utf8.RuneStart(value[start]) {
			start++
		}
		if start > 0 && start < len(value) && value[start-1] == '\r' && value[start] == '\n' {
			start++
		}
		return value[start:]
	}
	lo, hi := 0, min(len(value), bytes)
	for lo < hi {
		mid := lo + (hi-lo+1)/2
		if len(commandJSON(clip(mid)))-2 <= bytes {
			lo = mid
		} else {
			hi = mid - 1
		}
	}
	return clip(lo)
}
