// Package view binds product-owned templates to the shared Desktop renderer.
package view

import (
	"context"

	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
)

type (
	Session          = viewapi.Session
	SessionConfig    = viewapi.SessionConfig
	TerminalInfo     = viewapi.TerminalInfo
	Change           = viewapi.Change
	Receipt          = viewapi.Receipt
	SourceFile       = viewapi.SourceFile
	Destination      = viewapi.Destination
	Terminal         = viewapi.Terminal
	ConversationSpec = viewapi.ConversationSpec
	Conversation     = viewapi.Conversation
	ProviderAccount  = viewapi.ProviderAccount
	Services         = viewapi.Services
)

func WithServices(ctx context.Context, services Services) context.Context {
	return viewapi.WithServices(ctx, services)
}

func WindowServices(ctx context.Context) (Services, bool) {
	return viewapi.WindowServices(ctx)
}
