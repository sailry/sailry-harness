package view

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConversationToolObserver projects only the named calls of the mounted
// conversation, never calls from another session. Names also apply to the
// aliases of LegacyPackage when a saved conversation predates the module.
type ConversationToolObserver struct {
	Names         []string
	LegacyPackage string
	Publish       func(ConversationToolHistory)
}

type ConversationToolHistory struct {
	Connected bool                   `json:"connected"`
	Session   *protocol.SessionID    `json:"session,omitempty"`
	Revision  string                 `json:"revision,omitempty"`
	Latest    *string                `json:"latest,omitempty"`
	Calls     []ConversationToolCall `json:"calls"`
}

type ConversationToolCall struct {
	Key         string          `json:"key"`
	Name        string          `json:"name"`
	Arguments   json.RawMessage `json:"arguments"`
	State       string          `json:"state"`
	TimestampMs int64           `json:"timestamp_ms"`
	Result      json.RawMessage `json:"result"`
	Sequence    *string         `json:"sequence"`
}

// IsLegacyPackage recognizes a captured official package by its immutable content.
func IsLegacyPackage(name, digest string) bool { return plugins.IsLegacyDesktopPackage(name, digest) }
