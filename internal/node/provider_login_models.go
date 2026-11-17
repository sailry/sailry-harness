package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// discoverProviderLoginModels runs on the execution Node so every controller sees
// the same saved models, even if the controller that signed in disconnects.
func (node *Node) discoverProviderLoginModels(ctx context.Context, caller protocol.NodeID, provider protocol.ConversationProvider) error {
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "saved", Data: protocol.ConversationDiscoverySourceSavedPayload{
		Provider: provider.ID, ExpectedRevision: provider.Revision,
	}}})
	prepared, err := node.prepareProvider(ctx, caller, request)
	if err != nil {
		return err
	}
	if prepared == nil || prepared.Output == nil || prepared.Provider == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "account model discovery result is missing"}
	}
	catalog, ok := dispatchPayload[protocol.ConversationDiscoveryCatalog](prepared.Output.Data)
	if !ok || prepared.Output.Kind != "discovered_models" {
		return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "account model discovery result is invalid"}
	}
	var changes []protocol.Event
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		changes, err = store.CompleteProviderModelDiscovery(ctx, db, node.ID(), *prepared.Provider, catalog)
		if err == nil {
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.pluginCommitted(changes, ctx, nil)
	}
	return err
}
