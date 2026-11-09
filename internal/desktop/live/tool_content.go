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
			} else {
				payload = result["output"]
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
				out := &conversation.Diff{Path: diff.Path}
				for _, line := range strings.Split(diff.Text, "\n") {
					op := byte(' ')
					if len(line) > 0 && (line[0] == '+' || line[0] == '-' || line[0] == ' ') {
						op = line[0]
						line = line[1:]
					}
					out.Lines = append(out.Lines, conversation.DiffLine{Op: op, Text: line})
					if op == '+' {
						out.Added++
					}
					if op == '-' {
						out.Removed++
					}
				}
				call.Output.Diff = out
				call.Output.Partial = call.Output.Partial || diff.Truncated
			}
		}
	}
}
