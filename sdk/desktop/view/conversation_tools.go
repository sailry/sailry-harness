package view

import "github.com/sailry/sailry-harness/internal/desktop/viewapi"

type (
	ConversationToolObserver = viewapi.ConversationToolObserver
	ConversationToolHistory  = viewapi.ConversationToolHistory
	ConversationToolCall     = viewapi.ConversationToolCall
)

// IsLegacyPackage recognizes a captured official package by its immutable content.
func IsLegacyPackage(name, digest string) bool { return viewapi.IsLegacyPackage(name, digest) }
