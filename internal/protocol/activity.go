// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ActivityCatalog struct {
	Node      NodeID         `json:"node"`
	Cursor    uint64         `json:"cursor"`
	Projects  []Project      `json:"projects"`
	Worktrees []Worktree     `json:"worktrees"`
	Sessions  []Session      `json:"sessions"`
	Terminals []TerminalInfo `json:"terminals"`
}

func DefaultActivityCatalog() ActivityCatalog {
	return ActivityCatalog{
		Node:      NodeID{},
		Cursor:    0,
		Projects:  []Project{},
		Worktrees: []Worktree{},
		Sessions:  []Session{},
		Terminals: []TerminalInfo{},
	}
}
func (value *ActivityCatalog) UnmarshalJSON(data []byte) error {
	type plain ActivityCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "cursor", "projects", "worktrees", "sessions", "terminals"}, false); err != nil {
		return err
	}
	*value = ActivityCatalog(decoded)
	return nil
}
func (value ActivityCatalog) MarshalJSON() ([]byte, error) {
	type plain ActivityCatalog
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	encoded.Sessions = normalizeCollections(encoded.Sessions)
	encoded.Terminals = normalizeCollections(encoded.Terminals)
	return json.Marshal(encoded)
}

type ActivitySummary struct {
	Title     string            `json:"title"`
	Attention ActivityAttention `json:"attention"`
	Run       *ConversationRun  `json:"run"`
	Waiting   *ActivityWaiting  `json:"waiting"`
	Queued    uint32            `json:"queued"`
}

func DefaultActivitySummary() ActivitySummary {
	return ActivitySummary{
		Title:     "",
		Attention: DefaultActivityAttention(),
		Run:       nil,
		Waiting:   nil,
		Queued:    0,
	}
}
func (value *ActivitySummary) UnmarshalJSON(data []byte) error {
	type plain ActivitySummary
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"title", "attention", "queued"}, false); err != nil {
		return err
	}
	*value = ActivitySummary(decoded)
	return nil
}
func (value ActivitySummary) MarshalJSON() ([]byte, error) {
	type plain ActivitySummary
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ActivityPreview struct {
	Session SessionID `json:"session"`
	Turn    *TurnID   `json:"turn"`
	Text    *string   `json:"text"`
}

func DefaultActivityPreview() ActivityPreview {
	return ActivityPreview{
		Session: SessionID{},
		Turn:    nil,
		Text:    nil,
	}
}
func (value *ActivityPreview) UnmarshalJSON(data []byte) error {
	type plain ActivityPreview
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = ActivityPreview(decoded)
	return nil
}
func (value ActivityPreview) MarshalJSON() ([]byte, error) {
	type plain ActivityPreview
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ActivityWaiting string

const (
	ActivityWaitingApproval ActivityWaiting = "approval"
	ActivityWaitingInput    ActivityWaiting = "input"
)

func (value ActivityWaiting) MarshalJSON() ([]byte, error) {
	switch value {
	case ActivityWaitingApproval, ActivityWaitingInput:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ActivityWaiting %q", value)
	}
}
func (value *ActivityWaiting) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ActivityWaiting(decoded) {
	case ActivityWaitingApproval, ActivityWaitingInput:
		*value = ActivityWaiting(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ActivityWaiting %q", decoded)
	}
}
func (value ActivityWaiting) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ActivityWaiting) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ActivityAttention struct {
	Revision uint64 `json:"revision"`
	Unread   bool   `json:"unread"`
}

func DefaultActivityAttention() ActivityAttention {
	return ActivityAttention{
		Revision: 0,
		Unread:   false,
	}
}
func (value *ActivityAttention) UnmarshalJSON(data []byte) error {
	type plain ActivityAttention
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "unread"}, false); err != nil {
		return err
	}
	*value = ActivityAttention(decoded)
	return nil
}
func (value ActivityAttention) MarshalJSON() ([]byte, error) {
	type plain ActivityAttention
	encoded := plain(value)
	return json.Marshal(encoded)
}
