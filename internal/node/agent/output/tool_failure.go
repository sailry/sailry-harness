package output

import (
	"encoding/json"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// toolFailureText retains the actionable diagnostic from a known error. The
// full payload, including opaque runtime diagnostics, stays in history.
func toolFailureText(raw json.RawMessage) string {
	message := protocol.ToolErrorMessage(raw)
	if message == "" {
		return ""
	}
	value := object(decode(raw))
	var code, number, detail string
	collect := func(envelope map[string]any) {
		for _, fields := range []map[string]any{object(envelope["error"]), object(envelope["data"]), envelope} {
			switch v := fields["code"].(type) {
			case string:
				if code == "" && len(v) <= 64 && !strings.ContainsFunc(v, func(r rune) bool {
					return !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '_' || r == '-' || r == '.')
				}) {
					code = v
				}
			case json.Number:
				if number == "" && len(v) <= 24 {
					number = string(v)
				}
			}
			if detail == "" {
				for _, key := range []string{"details", "detail"} {
					if described := describeFailureDetail(fields[key], message); described != "" {
						detail = described
						break
					}
				}
			}
		}
	}
	collect(value)
	for _, key := range []string{"structuredContent", "data", "output"} {
		if nested := object(value[key]); nested != nil && protocol.ToolErrorMessage(json.RawMessage(encoded(nested))) == message {
			collect(nested)
		}
	}
	for _, part := range array(value["content"]) {
		fields := object(part)
		if fields["type"] != "text" {
			continue
		}
		nested := json.RawMessage(text(fields["text"]))
		if body := object(decode(nested)); body != nil && protocol.ToolErrorMessage(nested) == message {
			collect(body)
		}
	}
	if code == "" {
		code = number
	}
	if code != "" {
		message += "\nCode: " + code
	}
	if detail != "" {
		message += "\nDetails: " + detail
	}
	return message
}

func describeFailureDetail(value any, message string) string {
	if reason := diagnosticValue(value); reason != "" && reason != message {
		return reason
	}
	fields := object(value)
	var parts []string
	if kind := diagnosticValue(fields["kind"]); kind != "" {
		parts = append(parts, kind)
	}
	// Omit identifiers and arbitrary structured fields. Typed reasons and
	// recovery hints make sense on their own without the diagnostic envelope.
	for _, key := range []string{
		"reason", "message", "hint", "field", "parameter", "path", "required", "expected", "actual", "status", "cause", "phase",
		"exception", "operation", "context_overflow", "budget_ms", "elapsed_ms", "terminals_may_be_closed", "tasks_active",
	} {
		if summary := diagnosticValue(fields[key]); summary != "" && summary != message {
			parts = append(parts, key+"="+summary)
		}
	}
	result := strings.Join(parts, ", ")
	const limit = 512
	if len(result) > limit {
		return result[:headBytes(result, limit)] + "…"
	}
	return result
}

func diagnosticValue(value any) string {
	switch v := value.(type) {
	case string:
		summary := strings.TrimSpace(v)
		if strings.HasPrefix(summary, "{") || strings.HasPrefix(summary, "[") {
			return ""
		}
		const limit = 160
		if len(summary) > limit {
			return summary[:headBytes(summary, limit)] + "…"
		}
		return strings.Join(strings.Fields(summary), " ")
	case json.Number:
		if len(v) <= 24 {
			return string(v)
		}
	case bool:
		return strconv.FormatBool(v)
	}
	return ""
}
