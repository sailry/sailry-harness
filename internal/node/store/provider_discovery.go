package store

import (
	"context"
	"database/sql"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// CompleteProviderModelDiscovery adds an account's discovered models after sign-in.
// The captured revision keeps a late result from replacing edits or a new login.
// Existing models keep their configuration and adopt current native OAuth speed tiers.
func CompleteProviderModelDiscovery(ctx context.Context, db *sql.Conn, node protocol.NodeID, captured protocol.ConversationProvider, catalog protocol.ConversationDiscoveryCatalog) ([]protocol.Event, error) {
	provider := captured
	provider.Models = slices.Clone(captured.Models)
	for _, found := range catalog.Models {
		if index := slices.IndexFunc(provider.Models, func(model protocol.ConversationModel) bool { return model.ID == found.ID }); index >= 0 {
			provider.Models[index] = providers.RefreshAccountSpeed(provider, provider.Models[index], found.Capabilities)
			continue
		}
		model := protocol.DefaultConversationModel()
		model.ID, model.DisplayName, model.Capabilities = found.ID, found.DisplayName, found.Capabilities
		model.Context, model.Output = 128000, 16384
		if found.Context != nil {
			model.Context = *found.Context
		}
		if found.Output != nil {
			model.Output = *found.Output
		} else {
			model.Output = min(model.Output, model.Context)
		}
		provider.Models = append(provider.Models, model)
	}
	if len(provider.Models) > 0 && !slices.ContainsFunc(provider.Models, func(model protocol.ConversationModel) bool { return model.ID == provider.DefaultModel }) {
		provider.DefaultModel = provider.Models[0].ID
	}
	_, events, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{
		Provider: provider, ExpectedRevision: captured.Revision,
	}}}, nil)
	return events, err
}
