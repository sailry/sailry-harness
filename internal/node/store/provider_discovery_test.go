package store

import (
	"database/sql"
	"encoding/json"
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

func TestProviderLoginRefreshesAccountSpeed(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Authentication, provider.Api = protocol.AuthenticationChatGptOAuth, protocol.ConversationModelApiResponses
	provider.Endpoint = "https://chatgpt.com/backend-api/codex"

	stale := protocol.ConversationFeaturesFeature{ID: "speed", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "fast"}}}
	advertised := protocol.ConversationFeaturesFeature{ID: "speed", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "fast"}, {ID: "ultrafast"}}}
	other := protocol.ConversationFeaturesFeature{ID: "detail", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "full"}}}
	model := &provider.Models[0]
	model.DisplayName = new("Local display name")
	model.CustomCapabilities = true
	model.Capabilities = &protocol.ConversationDiscoveryCapabilities{
		Vision: new(false), Features: []protocol.ConversationFeaturesFeature{other, stale},
		Options: map[string]json.RawMessage{"fixture": json.RawMessage(`"configured"`)},
	}
	revoked, unmatched := *model, *model
	revoked.ID, unmatched.ID = "revoked-tier", "unmatched-model"
	revoked.Capabilities = &protocol.ConversationDiscoveryCapabilities{Features: []protocol.ConversationFeaturesFeature{stale}}
	provider.Models = append(provider.Models, revoked, unmatched)

	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{
			Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: provider},
		}}, nil)
		if err == nil {
			provider = output.Data.(protocol.ConversationProvider)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}

	want := append([]protocol.ConversationModel(nil), provider.Models...)
	updated := *want[0].Capabilities
	updated.Features = []protocol.ConversationFeaturesFeature{other, advertised}
	want[0].Capabilities = &updated
	removed := *want[1].Capabilities
	removed.Features = []protocol.ConversationFeaturesFeature{}
	want[1].Capabilities = &removed
	catalog := protocol.ConversationDiscoveryCatalog{Models: []protocol.ConversationDiscoveryModel{
		{ID: provider.Models[0].ID, Context: new(uint32(300000)), Capabilities: &protocol.ConversationDiscoveryCapabilities{Features: []protocol.ConversationFeaturesFeature{advertised}, Vision: new(true)}},
		{ID: revoked.ID, Capabilities: new(protocol.DefaultConversationDiscoveryCapabilities())},
	}}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		events, err := CompleteProviderModelDiscovery(ctx, db, node, provider, catalog)
		if err != nil {
			return err
		}
		if len(events) != 1 || events[0].Kind != "provider_changed" {
			return fmt.Errorf("refreshed model capabilities were not published")
		}
		saved := events[0].Data.(protocol.ConversationProvider)
		if saved.DefaultModel != provider.DefaultModel || saved.Revision != provider.Revision+1 || !reflect.DeepEqual(saved.Models, want) {
			return fmt.Errorf("refresh did not preserve the provider with updated speed choices: %+v", saved)
		}
		if !reflect.DeepEqual(provider.Models[0].Capabilities.Features, []protocol.ConversationFeaturesFeature{other, stale}) {
			return fmt.Errorf("refresh modified the captured provider")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
