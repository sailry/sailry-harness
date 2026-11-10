package live

import (
	"bytes"
	"encoding/json"
	"strings"

	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func projectToolContent(call *conversation.Call, loaded protocol.ToolDetail) {
	if resolved := loaded.Resolved; resolved != nil && resolved.Output != nil && resolved.Output.Body != nil {
		body := resolved.Output.Body
		if body.Diff != nil && *body.Diff == protocol.ToolDisplayProjectionFormatUnified {
			if call.Output == nil {
				call.Output = &conversation.Output{}
			}
			name := ""
			if body.Path != nil {
				name = *body.Path
			}
			call.Output.Diff = projectOutputDiff(name, body.Text)
			call.Output.Text = ""
			return
		}
	}
	if loaded.Result != nil {
		var result struct {
			Kind string `json:"kind"`
		}
		if json.Unmarshal(*loaded.Result, &result) == nil && (result.Kind == "command_result" || result.Kind == "command_output") {
			if call.Output == nil {
				call.Output = &conversation.Output{}
			}
			call.Output.Command = true
		}
	}
	if loaded.Content == nil {
		if call.Output == nil {
			call.Output = &conversation.Output{}
		}
		if call.Output.Text == "" {
			raw := loaded.Arguments
			if loaded.Result != nil {
				raw = *loaded.Result
			}
			var formatted bytes.Buffer
			if json.Indent(&formatted, raw, "", "  ") == nil {
				call.Output.Text = formatted.String()
			}
		}
		return
	}
	if call.Output == nil {
		call.Output = &conversation.Output{}
	}
	var payload any
	if loaded.Result != nil {
		json.Unmarshal(*loaded.Result, &payload)
	}
	if result, ok := payload.(map[string]any); ok {
		if _, direct := result["sailry_content"]; !direct {
			if structured, ok := result["structuredContent"].(map[string]any); ok && structured["sailry_content"] != nil {
				payload = structured
			} else if output, present := result["output"]; present {
				payload = output
			}
		}
	}
	firstText := true
	for _, block := range loaded.Content.Blocks {
		switch block.Kind {
		case "text":
			if text, ok := detail[protocol.ToolContentText](block.Data); ok {
				if value, err := jsonpointer.Value(payload, text.Path); err == nil {
					if body, ok := value.(string); ok {
						if firstText {
							call.Output.Text = ""
							firstText = false
						}
						if call.Output.Text != "" {
							call.Output.Text += "\n\n"
						}
						call.Output.Text += body
					}
				}
				for _, notice := range text.Notices {
					call.Output.Notices = append(call.Output.Notices, notice.Label)
				}
			}
		case "notice":
			if notice, ok := detail[protocol.ToolContentBlockNoticePayload](block.Data); ok {
				call.Output.Notices = append(call.Output.Notices, notice.Message.Label)
			}
		case "table":
			if table, ok := detail[protocol.ToolContentTable](block.Data); ok {
				call.Output.Table = &conversation.Table{Columns: table.Columns, Rows: table.Rows}
				call.Output.Partial = call.Output.Partial || table.Truncated
			}
		case "diff":
			if diff, ok := detail[protocol.ToolContentDiff](block.Data); ok {
				call.Output.Diff = projectOutputDiff(diff.Path, diff.Text)
				call.Output.Partial = call.Output.Partial || diff.Truncated
			}
		}
	}
}

func projectOutputDiff(path, text string) *conversation.Diff {
	out := &conversation.Diff{Path: path}
	unified := strings.HasPrefix(text, "@@ ") || strings.Contains(text, "\n@@ ")
	inside := !unified
	for _, line := range strings.Split(text, "\n") {
		if unified && strings.HasPrefix(line, "@@ ") {
			inside = true
			continue
		}
		if !inside || line == "" || strings.HasPrefix(line, "\\ No newline") {
			continue
		}
		op := byte(' ')
		if line[0] == '+' || line[0] == '-' || line[0] == ' ' {
			op, line = line[0], line[1:]
		} else if unified {
			continue
		}
		out.Lines = append(out.Lines, conversation.DiffLine{Op: op, Text: line})
		if op == '+' {
			out.Added++
		}
		if op == '-' {
			out.Removed++
		}
	}
	return out
}
