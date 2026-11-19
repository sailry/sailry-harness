package output

import (
	"encoding/json"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
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

// failureOnly reports whether a failed result carries nothing that its
// readable failure text would drop. A runtime fault is entirely diagnostic;
// a tool's own error keeps its other fields, extra parts and attachments.
func failureOnly(response *canonical.ToolResponse) bool {
	message := protocol.ToolErrorMessage(response.Result.Value)
	if response.Format == canonical.MCPError {
		return true
	}
	if len(response.Attachments) > 0 || len(response.Result.Assets) > 0 {
		return false
	}
	value := object(decode(response.Result.Value))
	if response.Format != canonical.MCP {
		return diagnosticOnly(value)
	}
	if value == nil {
		return false
	}
	for key, field := range value {
		switch key {
		case "isError", "_meta":
		case "structuredContent":
			if field != nil && !diagnosticOnly(object(field)) {
				return false
			}
		case "content":
			for _, part := range array(field) {
				fields := object(part)
				if fields["type"] != "text" {
					return false
				}
				body := strings.TrimSpace(text(fields["text"]))
				if body != message && !diagnosticOnly(object(decode(json.RawMessage(body)))) {
					return false
				}
			}
		default:
			return false
		}
	}
	return true
}

// diagnosticOnly accepts the error envelopes toolFailureText fully describes.
func diagnosticOnly(fields map[string]any) bool {
	if fields == nil {
		return false
	}
	for key, value := range fields {
		switch key {
		case "message", "code", "isError":
			if object(value) != nil || array(value) != nil {
				return false
			}
		case "error", "data", "output", "structuredContent":
			if nested := object(value); nested != nil && !diagnosticOnly(nested) || array(value) != nil {
				return false
			}
		case "details", "detail":
			if array(value) != nil {
				return false
			}
			for name, item := range object(value) {
				if object(item) != nil || array(item) != nil || name != "kind" && !slices.Contains(describedDetails, name) {
					return false
				}
			}
		default:
			return false
		}
	}
	return true
}

// Omit identifiers and arbitrary structured fields. Typed reasons and
// recovery hints make sense on their own without the diagnostic envelope.
var describedDetails = []string{
	"reason", "message", "hint", "field", "parameter", "path", "required", "expected", "actual", "status", "cause", "phase",
	"exception", "operation", "context_overflow", "budget_ms", "elapsed_ms", "terminals_may_be_closed", "tasks_active",
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
	for _, key := range describedDetails {
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
