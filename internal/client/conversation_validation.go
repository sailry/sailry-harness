package client

import (
	"encoding/json"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func validConversationRun(r protocol.ConversationRun, session protocol.SessionID) bool {
	if r.Session != session || r.Sequence == 0 {
		return false
	}
	if r.Origin != nil && (*r.Origin == session || activeConversationRun(r.Status)) {
		return false
	}
	if r.Input != nil {
		if r.Kind != "task" || r.Continuation != nil || r.Notice || len(r.Input.Message.Attachments) != len(r.Input.Attachments) {
			return false
		}
		for i, id := range r.Input.Message.Attachments {
			if id != r.Input.Attachments[i].ID {
				return false
			}
		}
	}
	return true
}

func validateConversationDraft(page protocol.ConversationPage, d protocol.ConversationDraft) error {
	if d.ID == "" || slices.ContainsFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.ID == d.ID }) || !slices.ContainsFunc(page.Runs, func(r protocol.ConversationRun) bool {
		return r.Turn == d.Turn && (r.Status == "running" || r.Status == "stopping")
	}) {
		return fault(protocol.ErrorCodeInvalidRequest, "streaming output does not belong to an active run")
	}
	for _, part := range d.Parts {
		switch part.Kind {
		case "text", "thinking":
			if _, err := payload[string](part.Data); err != nil {
				return err
			}
		case "resource":
			var raw struct {
				Type    string `json:"type"`
				Started *int64 `json:"started_ms"`
			}
			data, err := json.Marshal(part.Data)
			if err != nil || json.Unmarshal(data, &raw) != nil || raw.Type != "compacting" || raw.Started == nil {
				return fault(protocol.ErrorCodeInvalidRequest, "invalid streaming resource")
			}
		default:
			return fault(protocol.ErrorCodeInvalidRequest, "invalid streaming part")
		}
	}
	return nil
}

func validateConversationQueue(q protocol.ConversationQueue) error {
	if q.Revision == 0 && (q.Paused || len(q.Items) > 0) {
		return fault(protocol.ErrorCodeInvalidRequest, "queue snapshot is inconsistent")
	}
	turns := map[protocol.TurnID]bool{}
	for _, item := range q.Items {
		if item.Revision == 0 || item.ConfigRevision == 0 || turns[item.Turn] {
			return fault(protocol.ErrorCodeInvalidRequest, "queue snapshot is inconsistent")
		}
		turns[item.Turn] = true
	}
	return nil
}

func validateConversationApproval(page protocol.ConversationPage, a protocol.ConversationApproval) error {
	if a.Session != page.Session || a.Entry == "" {
		return fault(protocol.ErrorCodeInvalidRequest, "approval does not reference a canonical tool call")
	}
	for _, e := range page.Entries {
		if e.ID == a.Entry && (e.Turn != a.Turn || a.Index >= uint64(len(e.Parts)) || e.Parts[a.Index].Kind != "tool_call") {
			return fault(protocol.ErrorCodeInvalidRequest, "approval does not reference a canonical tool call")
		}
	}
	return nil
}

func checkApprovalTransition(a, b protocol.ConversationApproval, stale bool) error {
	terminal := a.State != "pending"
	allow := stale && b.State == "pending"
	if a.ID != b.ID || a.Session != b.Session || a.Turn != b.Turn || a.Entry != b.Entry || a.Index != b.Index || a.Source != b.Source || !reflect.DeepEqual(a.Access, b.Access) || !reflect.DeepEqual(a.Prefix, b.Prefix) || a.CanRemember != b.CanRemember || !reflect.DeepEqual(a.Startup, b.Startup) || !reflect.DeepEqual(a.Ssh, b.Ssh) || terminal && !allow && (a.State != b.State || !reflect.DeepEqual(a.Choice, b.Choice) || !reflect.DeepEqual(a.Reason, b.Reason)) {
		return fault(protocol.ErrorCodeInvalidRequest, "approval identity or terminal decision changed")
	}
	return nil
}

func checkQuestionTransition(a, b protocol.ConversationQuestion, stale bool) error {
	if a.ID != b.ID || a.Session != b.Session || a.Turn != b.Turn || a.Entry != b.Entry || a.Index != b.Index || a.State.Kind != "pending" && !(stale && b.State.Kind == "pending") && !reflect.DeepEqual(a.State, b.State) {
		return fault(protocol.ErrorCodeInvalidRequest, "question identity or terminal answer changed")
	}
	return nil
}

func validateConversationQuestion(page protocol.ConversationPage, q protocol.ConversationQuestion) error {
	if q.Session != page.Session || q.Entry == "" {
		return fault(protocol.ErrorCodeInvalidRequest, "question belongs to another conversation or has no call")
	}
	for _, entry := range page.Entries {
		if entry.ID != q.Entry {
			continue
		}
		if entry.Turn != q.Turn || q.Index >= uint64(len(entry.Parts)) || entry.Parts[q.Index].Kind != "tool_call" {
			return fault(protocol.ErrorCodeInvalidRequest, "question does not reference a canonical tool call")
		}
		call, err := payload[protocol.ConversationPartToolCallPayload](entry.Parts[q.Index].Data)
		if err != nil || call.Summary == nil || call.Summary.Question == nil || *call.Summary.Question == nil {
			return fault(protocol.ErrorCodeInvalidRequest, "question has invalid input")
		}
		spec := **call.Summary.Question
		if call.Name != "ask_user" && call.Name != "mcp_elicitation" && call.Name != "plan_review" || call.Name == "ask_user" && spec.Prompt == "" || call.Name == "plan_review" && (spec.Input.Kind != "plan" || spec.Prompt != "") || (call.Name == "mcp_elicitation") != (spec.Input.Kind == "form" || spec.Input.Kind == "url") || q.State.Kind == "declined" && spec.Input.Kind != "form" && spec.Input.Kind != "url" {
			return fault(protocol.ErrorCodeInvalidRequest, "question input does not match its source")
		}
		if q.State.Kind == "answered" {
			answer, err := payload[protocol.ConversationQuestionAnswer](q.State.Data)
			if err != nil {
				return err
			}
			if err = validateConversationAnswer(spec, answer); err != nil {
				return err
			}
		}
	}
	return nil
}

func validateConversationAnswer(spec protocol.ConversationQuestionSpec, a protocol.ConversationQuestionAnswer) error {
	valid := false
	switch spec.Input.Kind {
	case "text":
		if text, err := payload[string](a.Data); a.Kind == "text" && err == nil {
			input, e := payload[protocol.ConversationQuestionInputTextPayload](spec.Input.Data)
			valid = e == nil && strings.TrimSpace(text) != "" && uint64(len(text)) <= input.MaxBytes
		}
	case "choice":
		if input, e := payload[protocol.ConversationQuestionInputChoicePayload](spec.Input.Data); e == nil && a.Kind == "choices" {
			value, e := payload[protocol.ConversationQuestionAnswerChoicesPayload](a.Data)
			if e == nil {
				valid = (len(value.Selected) > 0 || value.Other != nil) && (input.Multiple || len(value.Selected) <= 1) && (value.Other == nil || input.AllowOther && strings.TrimSpace(*value.Other) != "")
				seen := map[uint64]bool{}
				for _, i := range value.Selected {
					if i >= uint64(len(input.Options)) || seen[i] {
						valid = false
					}
					seen[i] = true
				}
			}
		}
	case "plan":
		valid = a.Kind == "plan"
	case "url":
		valid = a.Kind == "opened"
	case "form":
		valid = a.Kind == "form"
	}
	if !valid {
		return fault(protocol.ErrorCodeInvalidRequest, "question answer does not match its input")
	}
	return nil
}

func mergeConversationChild(page *protocol.ConversationPage, c protocol.ConversationChild, keep bool) error {
	parent := slices.IndexFunc(page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == c.Origin.Turn })
	if parent < 0 {
		return fault(protocol.ErrorCodeInvalidRequest, "child has no parent turn")
	}
	p := page.Runs[parent]
	owner := page.Session
	if p.Origin != nil {
		owner = *p.Origin
	}
	if c.Origin.Session != owner || c.Run.Session == owner || c.Run.Session == page.Session || c.Run.Sequence <= p.Sequence || c.Run.Revision == 0 || c.Run.Kind != "task" || p.Kind != "task" || c.Run.Origin != nil || (c.Origin.Role != nil) != (c.Name != nil) || c.Name != nil && *c.Name == "" || slices.ContainsFunc(page.Runs, func(r protocol.ConversationRun) bool { return r.Turn == c.Run.Turn }) {
		return fault(protocol.ErrorCodeInvalidRequest, "child does not belong to its parent turn")
	}
	entry := slices.IndexFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.ID == c.Origin.Entry })
	if entry < 0 {
		return fault(protocol.ErrorCodeInvalidRequest, "child has no canonical delegation call")
	}
	e := page.Entries[entry]
	if e.Turn != p.Turn || c.Origin.Index >= uint64(len(e.Parts)) || e.Parts[c.Origin.Index].Kind != "tool_call" {
		return fault(protocol.ErrorCodeInvalidRequest, "child references another turn or tool")
	}
	call, err := payload[protocol.ConversationPartToolCallPayload](e.Parts[c.Origin.Index].Data)
	if err != nil || call.Summary == nil || call.Summary.Delegation.Invalid {
		return fault(protocol.ErrorCodeInvalidRequest, "child delegation selection is invalid")
	}
	d := call.Summary.Delegation
	if d.Session != nil && *d.Session != c.Run.Session {
		return fault(protocol.ErrorCodeInvalidRequest, "child session differs from its delegation")
	}
	worktree := p.Worktree
	if d.Session != nil {
		worktree = c.Run.Worktree
	}
	if d.Worktree != nil {
		worktree = *d.Worktree
	}
	if c.Run.Worktree != worktree {
		return fault(protocol.ErrorCodeInvalidRequest, "child worktree differs from its delegation")
	}
	for i, previous := range page.Children {
		if previous.Run.Session == c.Run.Session && (previous.Origin.Session != c.Origin.Session || !reflect.DeepEqual(previous.Origin.Role, c.Origin.Role) || !reflect.DeepEqual(previous.Name, c.Name) || previous.Run.Revision != c.Run.Revision || previous.Run.Worktree != c.Run.Worktree) || previous.Origin.Turn == c.Origin.Turn && previous.Origin.Entry == c.Origin.Entry && previous.Origin.Index == c.Origin.Index && previous.Run.Session != c.Run.Session {
			return fault(protocol.ErrorCodeInvalidRequest, "child ownership or frozen configuration changed")
		}
		if previous.Run.Turn != c.Run.Turn {
			if previous.Run.Sequence == c.Run.Sequence {
				return fault(protocol.ErrorCodeInvalidRequest, "child admission order repeats")
			}
			continue
		}
		if !reflect.DeepEqual(previous.Origin, c.Origin) || !reflect.DeepEqual(previous.Name, c.Name) || !sameRun(previous.Run, c.Run) || !activeConversationRun(previous.Run.Status) && !activeConversationRun(c.Run.Status) && !reflect.DeepEqual(previous.Run, c.Run) || !keep && (!activeConversationRun(previous.Run.Status) && !reflect.DeepEqual(previous.Run, c.Run) || previous.Run.Status == "stopping" && c.Run.Status == "running") {
			return fault(protocol.ErrorCodeInvalidRequest, "child identity or terminal state changed")
		}
		if !keep {
			page.Children[i] = c
		}
		return nil
	}
	page.Children = append(page.Children, c)
	slices.SortFunc(page.Children, func(a, b protocol.ConversationChild) int {
		if a.Run.Sequence < b.Run.Sequence {
			return -1
		}
		if a.Run.Sequence > b.Run.Sequence {
			return 1
		}
		return 0
	})
	return nil
}

func mergeConversationBackground(page *protocol.ConversationPage, t protocol.ConversationBackgroundTask) error {
	valid := false
	switch t.Kind.Kind {
	case "agent":
		if a, e := payload[protocol.ConversationBackgroundKindAgentPayload](t.Kind.Data); e == nil && a.Session != t.Session {
			_, err := protocol.ParseTurnID(strings.TrimPrefix(t.ID, "agent:"))
			valid = strings.HasPrefix(t.ID, "agent:") && err == nil
		}
	case "command":
		if c, e := payload[protocol.ConversationBackgroundKindCommandPayload](t.Kind.Data); e == nil {
			valid = t.ID == "command:"+c.ID.String() || t.Outcome != nil && *t.Outcome == "stalled" && t.ID == "stall:"+c.ID.String()
		}
	}
	if !valid || t.Session != page.Session || t.Revision == 0 {
		return fault(protocol.ErrorCodeInvalidRequest, "background task belongs to another scope")
	}
	for i, b := range page.Background {
		if b.ID != t.ID {
			continue
		}
		same := b.Session == t.Session && b.Turn == t.Turn && b.StartedMs == t.StartedMs && b.Kind.Kind == t.Kind.Kind
		if b.Kind.Kind == "agent" {
			a, _ := payload[protocol.ConversationBackgroundKindAgentPayload](b.Kind.Data)
			c, _ := payload[protocol.ConversationBackgroundKindAgentPayload](t.Kind.Data)
			same = same && a.Session == c.Session
		} else {
			same = same && reflect.DeepEqual(b.Kind, t.Kind)
		}
		if !same {
			return fault(protocol.ErrorCodeInvalidRequest, "background task source changed")
		}
		if t.Revision < b.Revision {
			return nil
		}
		if t.Revision == b.Revision && !reflect.DeepEqual(b, t) {
			return fault(protocol.ErrorCodeInvalidRequest, "background task changed without its revision")
		}
		page.Background[i] = t
		return nil
	}
	page.Background = append(page.Background, t)
	return nil
}
