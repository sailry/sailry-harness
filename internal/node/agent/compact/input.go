package compact

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

//go:embed prompt.md
var promptText string

const Request = "Please summarize the conversation history provided in the system prompt."

func Prepare(records []canonical.Record, contextLimit, outputLimit uint64, request ...string) (string, error) {
	records = Visible(records)
	filtered := records[:0]
	for _, record := range records {
		if record.Metadata.Runtime == nil || !record.Metadata.Runtime.TurnContext {
			filtered = append(filtered, record)
		}
	}
	records = filtered
	base := strings.TrimSpace(strings.ReplaceAll(promptText, "{{ messages }}", ""))
	requestReserve := uint64(256)
	if len(request) > 0 {
		requestReserve = max(requestReserve, tokens.Text(request[0])+7)
	}
	reserve := outputLimit + tokens.Text(base) + requestReserve
	if contextLimit < reserve {
		return "", fmt.Errorf("summary prompt and output exceed the context limit")
	}
	budget := contextLimit - reserve
	var turns [][]canonical.Record
	for _, record := range records {
		message := record.Message()
		kickoff := message != nil && message.Role == canonical.User
		if kickoff {
			hasInput, hasTool := false, false
			for _, block := range message.Content {
				hasInput = hasInput || block.Kind == "text" || block.Kind == "image" || block.Kind == "document"
				hasTool = hasTool || block.Kind == "tool_response" || block.Kind == "tool_request"
			}
			kickoff = hasInput && !hasTool
		}
		if len(turns) == 0 || kickoff {
			turns = append(turns, nil)
		}
		turns[len(turns)-1] = append(turns[len(turns)-1], record)
	}
	if len(turns) == 0 {
		return strings.TrimSpace(strings.ReplaceAll(promptText, "{{ messages }}", "")), nil
	}
	history := make([]string, len(turns))
	for i, turn := range turns {
		detail := 0
		if i == len(turns)-1 {
			detail = -1
		}
		history[i] = transcript(turn, detail)
	}
	// Each turn is measured once and again only when it is shortened.
	sizes := make([]uint64, len(history))
	var count uint64
	for i, part := range history {
		sizes[i] = tokens.Text(part) + 2
		count += sizes[i]
	}
	if sizes[len(sizes)-1]-2 > budget {
		return "", fmt.Errorf("latest turn exceeds the summary context limit")
	}
	fit := func() (string, bool) {
		if count > budget {
			return "", false
		}
		var nonempty []string
		for _, part := range history {
			if part != "" {
				nonempty = append(nonempty, part)
			}
		}
		body := strings.Join(nonempty, "\n")
		if tokens.Text(body) > budget {
			return "", false
		}
		return body, true
	}
	body, ok := fit()
	for detail := 1; !ok && detail <= 3; detail++ {
		for i := 0; i < len(turns)-1; i++ {
			history[i] = transcript(turns[i], detail)
			count -= sizes[i]
			sizes[i] = tokens.Text(history[i]) + 2
			count += sizes[i]
			body, ok = fit()
			if ok {
				break
			}
		}
	}
	if !ok {
		body = history[len(history)-1]
	}
	// The source summary request serializes its user transcript once more.
	body = "[user]: " + body
	return strings.TrimSpace(strings.ReplaceAll(promptText, "{{ messages }}", body)), nil
}
func transcript(records []canonical.Record, detail int) string {
	if detail == 3 {
		return ""
	}
	var parts []string
	for _, record := range records {
		message := record.Message()
		if message == nil {
			continue
		}
		if detail == 2 && (message.Role != canonical.User || hasTools(message)) {
			continue
		}
		for _, block := range message.Content {
			var text string
			switch value := block.Data.(type) {
			case *canonical.TextContent:
				text = value.Text
			case *canonical.AssetContent:
				text = "[" + block.Kind + ": " + value.Asset.Mime + "]"
			case *canonical.ToolRequest:
				if value.Error != nil {
					text = "tool_request: [error]"
				} else {
					text = "tool_request(" + value.Name + "): " + string(value.Arguments)
				}
			case *canonical.ToolResponse:
				if detail == 1 {
					text = "tool_response: [omitted]"
				} else if value.Format == canonical.MCPError {
					text = "tool_response: [error]"
				} else {
					text = "tool_response: " + resultText(value)
					if detail == 0 {
						text = "tool_response: " + edges(strings.TrimPrefix(text, "tool_response: "))
					}
				}
			default:
				continue
			}
			formatted := "[" + string(message.Role) + "]: " + text
			if detail == 1 && block.Kind == "tool_request" {
				formatted = edges(formatted)
			}
			parts = append(parts, formatted)
			if result, ok := block.Data.(*canonical.ToolResponse); ok && result.Format != canonical.MCPError && result.Revision != "" {
				reference, _ := json.Marshal(map[string]any{"output": map[string]string{"call": result.ID, "revision": result.Revision}, "next_offset": nil})
				parts = append(parts, string(reference))
			}
		}
		if message.Error != nil {
			parts = append(parts, "["+string(message.Role)+"]: error: "+message.Error.Message)
		}
	}
	return strings.Join(parts, "\n")
}
func hasTools(message *canonical.Message) bool {
	for _, block := range message.Content {
		if block.Kind == "tool_request" || block.Kind == "tool_response" {
			return true
		}
	}
	return false
}
func resultText(value *canonical.ToolResponse) string {
	if value.Format == canonical.JSON {
		return string(value.Result.Value)
	}
	var result struct {
		Content []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
	}
	if json.Unmarshal(value.Result.Value, &result) != nil {
		return "[non-text content]"
	}
	var texts []string
	for _, part := range result.Content {
		if part.Type == "text" {
			texts = append(texts, part.Text)
		}
	}
	if len(texts) == 0 {
		return "[non-text content]"
	}
	return strings.Join(texts, "\n")
}
func edges(text string) string {
	runes := []rune(text)
	if len(runes) <= 2000 {
		return text
	}
	return string(runes[:1000]) + fmt.Sprintf("\n[%d characters omitted]\n", len(runes)-2000) + string(runes[len(runes)-1000:])
}
