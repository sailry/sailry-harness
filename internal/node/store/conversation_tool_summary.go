package store

import (
	"bytes"
	"encoding/json"
	"reflect"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func agentJSON(data json.RawMessage) any {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return nil
	}
	return value
}
func agentPointer(value any, path string) any {
	if path == "" {
		return value
	}
	if !strings.HasPrefix(path, "/") {
		return nil
	}
	for _, part := range strings.Split(path[1:], "/") {
		part = strings.ReplaceAll(strings.ReplaceAll(part, "~1", "/"), "~0", "~")
		switch x := value.(type) {
		case map[string]any:
			value = x[part]
		case []any:
			index := 0
			if part == "" || len(part) > 1 && part[0] == '0' {
				return nil
			}
			for _, c := range part {
				if c < '0' || c > '9' || index > len(x) {
					return nil
				}
				index = index*10 + int(c-'0')
			}
			if index >= len(x) {
				return nil
			}
			value = x[index]
		default:
			return nil
		}
	}
	return value
}
func agentObject(value any) map[string]any { v, _ := value.(map[string]any); return v }
func agentString(value any) *string {
	v, ok := value.(string)
	if !ok {
		return nil
	}
	return &v
}
func agentClip(text string, limit int, single bool) string {
	var out strings.Builder
	for i, c := range []rune(text) {
		if i == limit || single && (c == '\r' || c == '\n') {
			break
		}
		out.WriteRune(c)
	}
	return out.String()
}
func agentLiteral(value any, arrays bool) *string {
	var text string
	switch x := value.(type) {
	case string:
		text = x
	case bool, json.Number:
		b, _ := json.Marshal(x)
		text = string(b)
	case []any:
		if !arrays {
			return nil
		}
		items := []string{}
		for _, item := range x {
			v, ok := item.(string)
			if !ok {
				return nil
			}
			items = append(items, v)
		}
		text = strings.Join(items, "\n")
	default:
		return nil
	}
	return &text
}
func (raw *rawAgentTool) value() any {
	value := raw.result()
	if raw.Format == canonical.MCP {
		if x, ok := agentObject(value)["structuredContent"]; ok {
			return x
		}
	}
	return value
}
func agentResultDisplay(value any) *protocol.ToolResultResultDisplay {
	raw, ok := agentObject(value)["sailry_result"]
	if !ok {
		return nil
	}
	body, _ := json.Marshal(raw)
	if len(body) > 2*1024*1024 {
		return nil
	}
	var result protocol.ToolResultResultDisplay
	if json.Unmarshal(body, &result) != nil || result.Version != 1 {
		return nil
	}
	if result.Status != nil && !validAgentNavigation(protocol.PluginDesktopNavigation{Label: result.Status.Label, Locales: result.Status.Locales}) {
		return nil
	}
	return &result
}
func (raw *rawAgentTool) errorText(metadata *protocol.ToolResultResultDisplay) *string {
	if metadata != nil && len(metadata.Diagnostics) > 0 {
		texts := []string{}
		for _, d := range metadata.Diagnostics {
			texts = append(texts, d.Text)
		}
		v := strings.Join(texts, "\n")
		return &v
	}
	if raw.Failure == nil {
		return nil
	}
	if raw.Error != nil {
		return raw.Error
	}
	value := raw.value()
	object := agentObject(value)
	for _, v := range []any{object["error"], object["data"]} {
		body, _ := json.Marshal(v)
		var fault protocol.Fault
		if json.Unmarshal(body, &fault) == nil && fault.Details != nil && fault.Details.Kind == "system_diagnostic" {
			return nil
		}
	}
	if v := agentString(agentPointer(value, "/error/message")); v != nil {
		return v
	}
	if v := agentString(object["error"]); v != nil {
		return v
	}
	switch raw.Format {
	case canonical.MCPError:
		return agentString(object["message"])
	case canonical.MCP:
		contents, _ := agentObject(raw.result())["content"].([]any)
		texts := []string{}
		for _, v := range contents {
			o := agentObject(v)
			if o["type"] == "text" {
				if t := agentString(o["text"]); t != nil {
					texts = append(texts, *t)
				}
			}
		}
		text := strings.Join(texts, "\n")
		if text == "" {
			return nil
		}
		parsed := agentJSON(json.RawMessage(text))
		if o, ok := parsed.(map[string]any); ok {
			if v := agentString(agentPointer(o, "/error/message")); v != nil {
				return v
			}
			if v := agentString(o["message"]); v != nil {
				return v
			}
			return agentString(o["error"])
		}
		return &text
	}
	return nil
}
func (raw *rawAgentTool) progress() *protocol.ConversationProgress {
	if raw.Presentation != protocol.ToolPresentationProgress {
		return nil
	}
	var expected protocol.ConversationProgress
	if json.Unmarshal(raw.Arguments, &expected) != nil || !ValidAgentProgress(expected) {
		return nil
	}
	value := agentObject(raw.value())
	if len(value) != 1 {
		return nil
	}
	body, _ := json.Marshal(value["progress"])
	var actual protocol.ConversationProgress
	if json.Unmarshal(body, &actual) != nil || !reflect.DeepEqual(expected, actual) {
		return nil
	}
	return &actual
}

// ValidAgentProgress reports whether a progress snapshot is non-empty and
// within the 64 KiB text budget.
func ValidAgentProgress(value protocol.ConversationProgress) bool {
	size := 0
	if value.Title != nil {
		if strings.TrimSpace(*value.Title) == "" {
			return false
		}
		size += len(*value.Title)
	}
	if len(value.Steps) == 0 {
		return false
	}
	for _, s := range value.Steps {
		if strings.TrimSpace(s.Description) == "" {
			return false
		}
		size += len(s.Description)
	}
	return size <= 64*1024
}
func (raw *rawAgentTool) delegation() protocol.ToolDetailDelegation {
	value := protocol.DefaultToolDetailDelegation()
	args := agentObject(agentJSON(raw.Arguments))
	if v, ok := args["session"]; ok {
		body, _ := json.Marshal(v)
		var id protocol.SessionID
		if json.Unmarshal(body, &id) != nil {
			value.Invalid = true
		} else {
			value.Session = &id
		}
	}
	if v, ok := args["worktree"]; ok && v != nil {
		body, _ := json.Marshal(v)
		var id protocol.WorktreeID
		if json.Unmarshal(body, &id) != nil {
			value.Invalid = true
		} else {
			value.Worktree = &id
		}
	}
	value.Background = raw.Delegation && args["background"] == true
	return value
}
func (raw *rawAgentTool) summary() (*protocol.ToolDetailSummary, error) {
	revision, err := raw.revision()
	if err != nil {
		return nil, err
	}
	value := protocol.DefaultToolDetailSummary()
	value.Revision = revision
	result := raw.value()
	metadata := agentResultDisplay(result)
	value.Resolved = raw.resolved(false)
	value.Failure = raw.Failure
	value.Diagnostics = raw.Failure != nil || metadata != nil && len(metadata.Diagnostics) > 0
	value.Error = raw.errorText(metadata)
	if value.Error != nil {
		clipped := agentClip(*value.Error, 1024, false)
		value.Error = &clipped
	}
	if metadata != nil {
		value.Expected = metadata.Expected
	}
	value.Progress = raw.progress()
	content := raw.content()
	visible := false
	if content != nil {
		visible = agentContentDetails(*content, result)
	} else if value.Resolved != nil && value.Resolved.Output != nil {
		out := value.Resolved.Output
		visible = out.PreviewDeferred || strings.TrimSpace(out.Preview) != "" || len(out.Notices) > 0 || len(out.Paths) > 0
		if raw.Display != nil && raw.Display.Output != nil {
			recipe := raw.Display.Output
			visible = visible || recipe.Table != nil
			if body := recipe.Body; body != nil {
				if text := agentString(agentReference(body.Text, agentJSON(raw.Arguments), result, nil)); text != nil {
					visible = visible || *text != "" || body.Empty != nil
				}
			}
		}
	} else if raw.Display == nil || raw.Display.Output == nil {
		literal := agentLiteral(result, true)
		visible = literal != nil && *literal != ""
	}
	summarized := raw.Presentation == protocol.ToolPresentationSummary || raw.Presentation == protocol.ToolPresentationHidden
	fullError := raw.errorText(metadata)
	failureDetails := value.Diagnostics && !(summarized && fullError != nil && len(*fullError) <= 1024)
	value.HasDetails = raw.Result != nil && (failureDetails || len(raw.Images) > 0 || value.Progress != nil || !summarized && (raw.Display != nil && raw.Display.RawResult || visible))
	if content != nil {
		for _, block := range content.Blocks {
			if block.Kind == "file" || block.Kind == "artifact" || block.Kind == "card" {
				value.Published = true
			}
		}
	}
	if raw.Display != nil {
		value.Icon = raw.Display.Icon
		value.RawResult = raw.Display.RawResult
	}
	value.Images = uint64(len(raw.Images))
	if raw.Result != nil {
		body, _ := json.Marshal(raw.result())
		n := uint64(len(body))
		value.Bytes = &n
	}
	if text := agentString(agentObject(result)["text"]); text != nil {
		n := uint64(strings.Count(*text, "\n"))
		if *text != "" && !strings.HasSuffix(*text, "\n") {
			n++
		}
		value.Lines = &n
	}
	args := agentObject(agentJSON(raw.Arguments))
	if path := agentString(args["path"]); path != nil && *path != "" && !strings.ContainsAny(*path, "\r\n") {
		v := agentClip(*path, 200, true)
		value.Path = &v
	}
	for _, item := range []struct {
		v    any
		dest **string
	}{{args["title"], &value.Title}, {args["skill"], &value.Resource}, {agentObject(result)["summary_id"], &value.SummaryID}} {
		if v := agentString(item.v); v != nil && strings.TrimSpace(*v) != "" {
			text := agentClip(*v, 200, true)
			*item.dest = &text
		}
	}
	value.Delegation = raw.delegation()
	if raw.Name == "ask_user" || raw.Name == "mcp_elicitation" || raw.Name == "plan_review" {
		var spec protocol.ConversationQuestionSpec
		if json.Unmarshal(raw.Arguments, &spec) == nil && validateQuestionSpec(spec) == nil {
			p := &spec
			value.Question = &p
		}
	}
	if value.Resolved != nil {
		boundAgentResolved(value.Resolved)
	}
	return &value, nil
}
