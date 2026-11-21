package store

import (
	"bytes"
	"database/sql"
	"math"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderBillingRateSave(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Api = protocol.ConversationModelApiResponses
	api, account := protocol.AuthenticationApiKey, protocol.AuthenticationChatGptOAuth
	zero, full, maxRate := uint32(0), protocol.DefaultBillingRateBps, uint32(math.MaxUint32)
	for _, tt := range []struct {
		name, command string
		auth          protocol.Authentication
		input, want   *uint32
		reject        bool
	}{
		{"legacy record", "put_provider", api, nil, nil, false},
		{"save maximum", "save_provider", api, &maxRate, &maxRate, false},
		{"older put keeps maximum", "put_provider", api, nil, &maxRate, false},
		{"put explicit zero", "put_provider", api, &zero, &zero, false},
		{"partial save keeps zero", "save_provider", api, nil, &zero, false},
		{"explicit reset to 100%", "save_provider", api, &full, &full, false},
		{"account save rejects 100%", "save_provider", account, &full, nil, true},
		{"account put rejects zero", "put_provider", account, &zero, nil, true},
		{"change to account drops rate", "put_provider", account, nil, nil, false},
		{"change back to API does not resurrect rate", "save_provider", api, nil, nil, false},
	} {
		edit := provider
		edit.Name, edit.Authentication, edit.BillingRateBps = tt.name, tt.auth, tt.input
		command := protocol.Command{Kind: tt.command}
		if tt.command == "save_provider" {
			command.Data = protocol.CommandSaveProviderPayload{Provider: edit, ExpectedRevision: provider.Revision}
		} else {
			command.Data = protocol.CommandPutProviderPayload{Provider: edit, ExpectedRevision: provider.Revision}
		}
		var output protocol.Output
		var events []protocol.Event
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			output, events, err = ExecuteProviderCommand(ctx, db, node, protocol.NewRequest(node, command), nil)
			return err
		})
		if tt.reject {
			if !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				t.Fatalf("%s: got %v, want invalid request", tt.name, err)
			}
			continue
		}
		if err != nil {
			t.Fatalf("%s: %v", tt.name, err)
		}
		provider = output.Data.(protocol.ConversationProvider)
		if provider.Name != tt.name || !reflect.DeepEqual(provider.BillingRateBps, tt.want) {
			t.Fatalf("%s: save response did not keep the expected settings", tt.name)
		}
		if len(events) == 0 || events[0].Kind != "provider_changed" || !reflect.DeepEqual(events[0].Data.(protocol.ConversationProvider).BillingRateBps, tt.want) {
			t.Fatalf("%s: provider change did not keep the expected rate", tt.name)
		}
		var listed []protocol.ConversationProvider
		var captured protocol.ConversationProvider
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			listed, _, err = ReadProviderSnapshot(ctx, db, node)
			if err == nil && len(listed) == 1 {
				captured, err = CaptureProvider(ctx, db, listed[0])
			}
			return err
		}); err != nil {
			t.Fatalf("%s: %v", tt.name, err)
		}
		if len(listed) != 1 || !reflect.DeepEqual(listed[0].BillingRateBps, tt.want) || !reflect.DeepEqual(captured.BillingRateBps, tt.want) {
			t.Fatalf("%s: stored or captured provider did not keep the expected rate", tt.name)
		}
	}
}

func TestProviderBillingRateProjectionIgnoresStoredAccountRate(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationChatGptOAuth
	rate := uint32(20_000)
	provider.BillingRateBps = &rate
	body, err := encode(providerConfiguration{ID: provider.ID, Provider: &provider, Listed: true})
	if err != nil {
		t.Fatal(err)
	}
	schemaExecute(t, storage.Node, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
	var captured protocol.ConversationProvider
	var listed []protocol.ConversationProvider
	var stored []byte
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		listed, _, err = ReadProviderSnapshot(ctx, db, node)
		if err != nil {
			return err
		}
		captured, err = CaptureProvider(ctx, db, provider)
		if err != nil {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT body FROM providers WHERE id=?1", provider.ID.String()).Scan(&stored)
	}); err != nil {
		t.Fatal(err)
	}
	if len(listed) != 1 || listed[0].BillingRateBps != nil || captured.BillingRateBps != nil {
		t.Fatal("a non-API provider projected a billing rate")
	}
	if !bytes.Equal(stored, body) {
		t.Fatal("reading the projection changed its stored record")
	}
}
