package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderLoginModelDiscoveryPreservesConfiguration(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Models[0].CustomCapabilities = true
	provider.Models[0].Vision = false
	provider.DefaultModel = provider.Models[0].ID
	var captured protocol.ConversationProvider
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: provider}}}, nil)
		if err == nil {
			captured = output.Data.(protocol.ConversationProvider)
		}
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	context, output := uint32(240000), uint32(32000)
	catalog := protocol.ConversationDiscoveryCatalog{Models: []protocol.ConversationDiscoveryModel{
		{ID: captured.Models[0].ID, Context: &context, Output: &output},
		{ID: "new-account-model", Context: &context, Output: &output},
	}}
	var saved protocol.ConversationProvider
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		events, err := CompleteProviderModelDiscovery(ctx, db, node, captured, catalog)
		if err == nil {
			if len(events) != 1 || events[0].Kind != "provider_changed" {
				return fmt.Errorf("discovered models were not published")
			}
			saved = events[0].Data.(protocol.ConversationProvider)
		}
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.Models) != len(captured.Models)+1 || !reflect.DeepEqual(saved.Models[:len(captured.Models)], captured.Models) || saved.DefaultModel != captured.DefaultModel {
		t.Fatalf("discovery replaced configured models or their default: %+v", saved)
	}
	last := saved.Models[len(saved.Models)-1]
	if last.ID != "new-account-model" || last.Context != context || last.Output != output {
		t.Fatalf("discovered model metadata was lost: %+v", last)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := CompleteProviderModelDiscovery(ctx, db, node, captured, catalog)
		return err
	})
	if !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("late discovery overwrote a newer provider revision: %v", err)
	}
}
