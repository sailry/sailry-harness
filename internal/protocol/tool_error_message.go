package protocol

import (
	"encoding/json"
	"strings"
)

// ToolErrorMessage extracts the readable message of a known tool failure.
// Callers must establish failure independently of the payload's contents.
// An empty result leaves the original payload available as the fallback.
func ToolErrorMessage(raw json.RawMessage) string {
	var envelope toolErrorEnvelope
	if json.Unmarshal(raw, &envelope) != nil {
		return ""
	}
	if message := envelope.message(); message != "" {
		return message
	}
	var reason string
	for _, payload := range []json.RawMessage{envelope.StructuredContent, envelope.Data, envelope.Output} {
		var nested toolErrorEnvelope
		if json.Unmarshal(payload, &nested) == nil {
			if message := nested.message(); message != "" {
				if reason != "" && reason != message {
					return ""
				}
				reason = message
			}
		}
	}
	if reason != "" {
		return reason
	}
	var parts []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(envelope.Content, &parts) != nil {
		return ""
	}
	plain, texts := "", 0
	for _, part := range parts {
		message := strings.TrimSpace(part.Text)
		if part.Type != "text" || message == "" {
			continue
		}
		texts++
		switch {
		case strings.HasPrefix(message, "{"):
			var nested toolErrorEnvelope
			if json.Unmarshal([]byte(message), &nested) == nil {
				if explicit := nested.message(); explicit != "" {
					if reason != "" && reason != explicit {
						return ""
					}
					reason = explicit
				}
			}
		case !json.Valid([]byte(message)):
			plain = message
		}
	}
	if reason == "" && texts == 1 {
		return plain
	}
	return reason
}

type toolErrorEnvelope struct {
	Message           json.RawMessage `json:"message"`
	Error             json.RawMessage `json:"error"`
	Data              json.RawMessage `json:"data"`
	Output            json.RawMessage `json:"output"`
	StructuredContent json.RawMessage `json:"structuredContent"`
	Content           json.RawMessage `json:"content"`
}

func (e toolErrorEnvelope) message() string {
	var nested struct {
		Message json.RawMessage `json:"message"`
	}
	if json.Unmarshal(e.Error, &nested) == nil {
		if message := toolErrorString(nested.Message); message != "" {
			return message
		}
	}
	if message := toolErrorString(e.Message); message != "" {
		return message
	}
	return toolErrorString(e.Error)
}

func toolErrorString(raw json.RawMessage) string {
	var text string
	if json.Unmarshal(raw, &text) != nil {
		return ""
	}
	return strings.TrimSpace(text)
}
