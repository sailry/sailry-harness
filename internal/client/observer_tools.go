package client

import (
	"bytes"
	"encoding/json"
	"reflect"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type projectedToolCall struct {
	Turn         protocol.TurnID
	ID           *string
	Name         string
	Presentation protocol.ToolPresentation
	Grouping     protocol.ToolGrouping
	Source       toolReference
	Response     *toolReference
	State        string
	Summary      *protocol.ToolDetailSummary
	Approval     *protocol.ConversationApproval
	Question     *protocol.ConversationQuestion
	Reference    *protocol.ToolDetailReference
	Images       []protocol.ConversationImage
}

type callIdentity struct {
	turn   protocol.TurnID
	branch string
	id     string
}

func collectToolCalls(page protocol.ConversationPage) []projectedToolCall {
	calls := []projectedToolCall{}
	pending := map[callIdentity][]int{}
	for _, entry := range page.Entries {
		for index, part := range entry.Parts {
			source := toolReference{Entry: entry.ID, Index: uint64(index)}
			switch part.Kind {
			case "tool_call":
				value, err := payload[protocol.ConversationPartToolCallPayload](part.Data)
				if err != nil {
					continue
				}
				if value.ID != nil {
					key := callIdentity{turn: entry.Turn, branch: entry.Branch, id: *value.ID}
					pending[key] = append(pending[key], len(calls))
				}
				call := projectedToolCall{
					Turn: entry.Turn, ID: value.ID, Name: value.Name, Presentation: value.Presentation, Grouping: value.Grouping,
					Source: source, State: "waiting", Summary: value.Summary,
				}
				for _, approval := range page.Approvals {
					if approval.Turn == entry.Turn && approval.Entry == entry.ID && approval.Index == uint64(index) {
						owned := approval
						call.Approval = &owned
						break
					}
				}
				for _, question := range page.Questions {
					if question.Turn == entry.Turn && question.Entry == entry.ID && question.Index == uint64(index) {
						owned := question
						call.Question = &owned
						break
					}
				}
				calls = append(calls, call)
			case "resource":
				value := resourceValue(part.Data)
				if value.Type != "tool_started" {
					continue
				}
				indices := pending[callIdentity{turn: entry.Turn, branch: entry.Branch, id: value.ID}]
				if len(indices) == 1 && calls[indices[0]].Name == value.Name {
					calls[indices[0]].State = "running"
				}
			case "tool_result":
				value, err := payload[protocol.ConversationPartToolResultPayload](part.Data)
				if err != nil {
					continue
				}
				var indices []int
				if value.ID != nil {
					key := callIdentity{turn: entry.Turn, branch: entry.Branch, id: *value.ID}
					indices = pending[key]
					delete(pending, key)
				}
				if len(indices) == 1 && calls[indices[0]].Name == value.Name {
					call := &calls[indices[0]]
					call.Response, call.State, call.Summary, call.Images = &source, "returned", value.Summary, value.Images
				} else {
					calls = append(calls, projectedToolCall{
						Turn: entry.Turn, ID: value.ID, Name: value.Name, Presentation: "summary", Grouping: "sequence",
						Source: source, Response: &source, State: "returned", Summary: value.Summary, Images: value.Images,
					})
				}
			}
		}
	}
	for index := range calls {
		call := &calls[index]
		if call.Presentation == "progress" {
			call.Grouping = "standalone"
		}
		if call.Summary != nil {
			source := call.Source
			if call.Response != nil {
				source = *call.Response
			}
			call.Reference = &protocol.ToolDetailReference{Session: page.Session, Turn: call.Turn, Entry: source.Entry, Index: source.Index, Call: call.ID, Revision: call.Summary.Revision}
		}
		if call.State != "waiting" && call.State != "running" {
			continue
		}
		status := protocol.ConversationStatus("")
		for _, run := range page.Runs {
			if run.Turn == call.Turn {
				status = run.Status
				break
			}
		}
		if status == "running" || status == "stopping" || status == "queued" {
			continue
		}
		if status == "completed" && completionReview(page, *call) {
			call.State = "waiting"
		} else if status == "cancelled" && (call.Approval == nil || call.Approval.State != "approved") &&
			(call.Question == nil || call.Question.State.Kind != "answered") {
			call.State = "cancelled"
		} else if call.State == "waiting" && call.Approval != nil && (call.Approval.State == "interrupted" || call.Approval.State == "cancelled") {
			call.State = "not_executed"
		} else {
			call.State = "interrupted"
		}
	}
	return calls
}

func completionReview(page protocol.ConversationPage, call projectedToolCall) bool {
	if call.Name != "plan_review" || call.Response != nil || call.Question == nil || call.Question.State.Kind != "pending" {
		return false
	}
	validRun, validEntry := false, false
	for _, run := range page.Runs {
		validRun = validRun || run.Turn == call.Turn && run.Kind == "task" && run.Status == "completed" && run.Error == nil
	}
	for _, entry := range page.Entries {
		if entry.ID == call.Source.Entry && entry.Author == "plan_review" && call.Source.Index < uint64(len(entry.Parts)) {
			part, err := payload[protocol.ConversationPartToolCallPayload](entry.Parts[call.Source.Index].Data)
			validEntry = err == nil && part.Summary != nil && part.Summary.Question != nil && *part.Summary.Question != nil &&
				(**part.Summary.Question).Prompt == "" && (**part.Summary.Question).Input.Kind == "plan"
		}
	}
	return validRun && validEntry
}

func terminalInteraction(name string) bool {
	return name == "ask_user" || name == "plan_review" || name == "mcp_elicitation"
}

func toolHidden(call projectedToolCall) bool {
	return call.Presentation == "hidden" && call.State == "returned" && call.Approval == nil && call.Question == nil &&
		!terminalInteraction(call.Name) && (call.Summary == nil || !call.Summary.Diagnostics) && len(call.Images) == 0
}

func toolHasDetails(call projectedToolCall) bool {
	return (call.Approval == nil || call.Approval.State != "denied") && call.Summary != nil && call.Summary.HasDetails
}

func projectToolCall(page protocol.ConversationPage, call projectedToolCall) map[string]any {
	value := map[string]any{
		"turn": call.Turn, "id": call.ID, "name": call.Name, "terminal_interaction": terminalInteraction(call.Name),
		"presentation": call.Presentation, "grouping": call.Grouping, "source": call.Source, "response": call.Response,
		"state": call.State, "approval": call.Approval, "question": call.Question, "detail_state": nil, "detail": nil,
		"children": associatedChildren(page, call), "row": toolRow(call), "approval_view": toolApprovalView(call, nil),
		"revision": nil, "detail_key": nil, "published": false, "hidden": toolHidden(call), "details": toolHasDetails(call), "output": nil,
	}
	if call.Reference != nil {
		value["revision"], value["detail_key"] = call.Reference.Revision, detailKey(*call.Reference)
	}
	if call.Summary != nil {
		summary := call.Summary
		value["published"] = summary.Published
		if summary.Failure != nil {
			value["failure"] = summary.Failure
		}
		if summary.Progress != nil {
			value["progress"] = summary.Progress
		}
		if summary.Resolved != nil {
			value["resolved"] = summary.Resolved
			if !toolHasDetails(call) && !summary.Diagnostics {
				value["output"] = summary.Resolved.Output
			}
		}
	}
	return value
}

func toolRow(call projectedToolCall) map[string]any {
	var label *protocol.ToolResultLocalText
	var icon *protocol.PluginDesktopIcon
	var detail *protocol.ToolDetailText
	var state any
	summary := call.Summary
	var code *protocol.ErrorCode
	var failure *protocol.ToolFailure
	if summary != nil {
		icon = summary.Icon
		if summary.Resolved != nil {
			label, detail = summary.Resolved.Label, summary.Resolved.Input.Summary
		}
		failure = summary.Failure
		if failure != nil {
			code = failure.Code
		}
		if code != nil && *code == "not_found" && summary.Expected != nil || code != nil && (*code == "cancelled" || *code == "outcome_unknown") {
			failure = nil
		}
		if detail == nil && summary.Path != nil {
			detail = &protocol.ToolDetailText{Text: *summary.Path}
		}
	}
	denied := call.Approval != nil && call.Approval.State == "denied"
	pending := call.Approval != nil && call.Approval.State == "pending"
	failed, warning := failure != nil && failure.Origin == "runtime", failure != nil && failure.Origin == "tool"
	if denied {
		state = "denied"
	} else if code != nil && *code == "cancelled" {
		state = nil
	} else if code != nil && *code == "outcome_unknown" {
		state = "interrupted"
	} else if failed {
		state = "failed"
	} else if warning {
		state = nil
	} else if pending {
		state = "pending"
	} else if call.State == "not_executed" || call.State == "interrupted" {
		state = call.State
	}
	running := call.State == "running" && !denied && failure == nil && !pending
	if running {
		state = nil
	}
	if (failed || warning) && !denied && summary != nil && summary.Error != nil && strings.TrimSpace(*summary.Error) != "" {
		detail = &protocol.ToolDetailText{Text: *summary.Error}
	}
	return map[string]any{
		"label": label, "icon": icon, "detail": detail, "state": state, "failed": !denied && failed,
		"warning": !failed && (warning || state == "denied" || state == "interrupted" || state == "not_executed"),
		"pending": pending, "running": running, "has_details": toolHasDetails(call),
	}
}

func toolApprovalView(call projectedToolCall, resolved *protocol.ToolDetailResolved) map[string]any {
	value := map[string]any{
		"prompt": nil, "context": nil, "description": nil, "unrestricted": false, "notices": []protocol.ToolResultLocalText{}, "code": false,
	}
	if call.Approval != nil {
		value["unrestricted"] = call.Approval.Access != nil && *call.Approval.Access == "unrestricted"
		if call.Approval.Changes != nil {
			value["changes"] = call.Approval.Changes
		}
	}
	if resolved == nil && call.Summary != nil {
		resolved = call.Summary.Resolved
	}
	if resolved == nil || call.Summary == nil {
		return value
	}
	var description *protocol.ToolResultLocalText
	if len(resolved.Approval) > 0 {
		description = &resolved.Approval[0]
	} else {
		target := resolved.Input.Target
		if target == nil {
			target = resolved.Input.Summary
		}
		if target != nil {
			description = &protocol.ToolResultLocalText{Label: target.Text, Locales: map[string]string{}}
		}
	}
	prompt := description
	if prompt == nil {
		prompt = resolved.Label
	}
	value["prompt"], value["description"] = prompt, description
	if resolved.Input.Context != nil {
		value["context"] = resolved.Input.Context.Text
	}
	value["code"] = resolved.Input.Summary != nil && resolved.Input.Summary.Code && description != nil && description.Label == resolved.Input.Summary.Text
	notices := []protocol.ToolResultLocalText{}
	for _, notice := range resolved.Approval {
		if description == nil || !reflect.DeepEqual(notice, *description) {
			notices = append(notices, notice)
		}
	}
	value["notices"] = notices
	return value
}

func toolDetailPacket(detail protocol.ToolDetail, call projectedToolCall) map[string]any {
	resolved := detail.Resolved
	if resolved == nil && call.Summary != nil {
		resolved = call.Summary.Resolved
	}
	var output *protocol.ToolDetailOutput
	if resolved != nil {
		output = resolved.Output
	}
	if output == nil && detail.Result != nil {
		if text := literalToolResult(*detail.Result); text != nil {
			value := protocol.DefaultToolDetailOutput()
			value.Preview = *text
			output = &value
		}
	}
	if call.Summary != nil && call.Summary.Diagnostics {
		output = nil
	}
	texts := []any{}
	if detail.Content != nil {
		for _, block := range detail.Content.Blocks {
			var text *string
			if block.Kind == "text" && detail.Result != nil {
				if value, err := payload[protocol.ToolContentText](block.Data); err == nil {
					text = toolContentText(*detail.Result, value.Path)
				}
			}
			texts = append(texts, text)
		}
	}
	return map[string]any{
		"arguments": detail.Arguments, "result": detail.Result, "resolved": resolved, "content": detail.Content, "texts": texts, "output": output,
	}
}

func literalToolResult(data json.RawMessage) *string {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return nil
	}
	var text string
	switch value := value.(type) {
	case string:
		text = value
	case json.Number:
		text = value.String()
	case bool:
		text = strconv.FormatBool(value)
	case []any:
		parts := make([]string, 0, len(value))
		for _, item := range value {
			part, ok := item.(string)
			if !ok {
				return nil
			}
			parts = append(parts, part)
		}
		text = strings.Join(parts, "\n")
	}
	if text == "" {
		return nil
	}
	return &text
}

func toolContentText(data json.RawMessage, path string) *string {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return nil
	}
	if object, ok := value.(map[string]any); ok {
		if _, found := object["sailry_content"]; !found {
			if structured, ok := object["structuredContent"].(map[string]any); ok && structured["sailry_content"] != nil {
				value = structured
			} else {
				value = object["output"]
			}
		}
	} else {
		return nil
	}
	if path != "" {
		if !strings.HasPrefix(path, "/") {
			return nil
		}
		for _, field := range strings.Split(path[1:], "/") {
			field = strings.ReplaceAll(strings.ReplaceAll(field, "~1", "/"), "~0", "~")
			switch current := value.(type) {
			case map[string]any:
				value = current[field]
			case []any:
				index, err := strconv.ParseUint(field, 10, 64)
				if err != nil || index >= uint64(len(current)) {
					return nil
				}
				value = current[index]
			default:
				return nil
			}
		}
	}
	if text, ok := value.(string); ok {
		return &text
	}
	return nil
}

type resource struct {
	Type      string          `json:"type"`
	ID        string          `json:"id"`
	Name      string          `json:"name"`
	Attempt   uint64          `json:"attempt"`
	Limit     *uint64         `json:"limit"`
	Transport *string         `json:"transport"`
	Fallback  bool            `json:"fallback"`
	Failure   json.RawMessage `json:"failure"`
	StartedMs *int64          `json:"started_ms"`
}

func resourceValue(value any) resource {
	data, _ := json.Marshal(value)
	var result resource
	if json.Unmarshal(data, &result) != nil {
		return resource{}
	}
	if result.Type == "model_retry" && !(result.Fallback && result.Attempt == 0 && result.Limit != nil && *result.Limit <= 100 ||
		result.Attempt > 0 && (result.Limit == nil || *result.Limit <= 100 && result.Attempt <= *result.Limit)) {
		return resource{}
	}
	if result.Transport != nil && *result.Transport != "http" {
		return resource{}
	}
	return result
}

func projectChildren(page protocol.ConversationPage) map[string]any {
	value := map[string]any{}
	for _, child := range LatestConversationChildren(page.Children) {
		for index, entry := range page.Children {
			if reflect.DeepEqual(entry, child) {
				value[child.Run.Session.String()] = map[string]any{"index": index, "target": childTarget(child)}
				break
			}
		}
	}
	return value
}

func associatedChildren(page protocol.ConversationPage, call projectedToolCall) []map[string]any {
	matching := []protocol.ConversationChild{}
	for _, child := range page.Children {
		if child.Origin.Session == page.Session && child.Origin.Turn == call.Turn && child.Origin.Entry == call.Source.Entry && child.Origin.Index == call.Source.Index {
			matching = append(matching, child)
		}
	}
	value := []map[string]any{}
	for _, child := range LatestConversationChildren(matching) {
		for index, entry := range page.Children {
			if reflect.DeepEqual(entry, child) {
				value = append(value, map[string]any{"index": index, "target": childTarget(child)})
				break
			}
		}
	}
	return value
}
