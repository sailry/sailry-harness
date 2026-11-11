package engine

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RecordSearchUsage completes the billed response append after cancellation.
func (r *Runtime) RecordSearchUsage(ctx context.Context, session protocol.SessionID, turn protocol.TurnID, provider protocol.ConversationProvider, name string, usage canonical.Usage) error {
	message := canonical.NewMessage(canonical.Assistant)
	message.Model = &name
	message.Usage = &usage
	record := canonical.NewRecord(turn, "message", &message, time.Now())
	record.Provenance.Author = "system"
	purpose := canonical.PurposeSearch
	record.Metadata.Execution = &canonical.Execution{Purpose: &purpose, Source: &canonical.ModelSource{ID: provider.ID, Name: provider.Name, API: provider.Api, Authentication: provider.Authentication, Model: name}}
	record.Metadata.Runtime = &canonical.Runtime{UserVisible: false, AgentVisible: false}
	return r.save(context.WithoutCancel(ctx), session, record)
}

// RecordMediaUsage records one media call under the provider and model that
// actually ran it, after cancellation as well.
func (r *Runtime) RecordMediaUsage(ctx context.Context, session protocol.SessionID, turn protocol.TurnID, provider protocol.ConversationProvider, name string, kind protocol.MediaKind, usage canonical.Usage) error {
	message := canonical.NewMessage(canonical.Assistant)
	message.Model = &name
	message.Usage = &usage
	record := canonical.NewRecord(turn, "message", &message, time.Now())
	record.Provenance.Author = "system"
	record.Metadata.Execution = &canonical.Execution{Media: &kind, Source: &canonical.ModelSource{ID: provider.ID, Name: provider.Name, API: provider.Api, Authentication: provider.Authentication, Model: name}}
	record.Metadata.Runtime = &canonical.Runtime{UserVisible: false, AgentVisible: false}
	return r.save(context.WithoutCancel(ctx), session, record)
}
