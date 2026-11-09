package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func providerStoreFixtures(t *testing.T) {
	t.Run("provider_inline_authorization_json", func(t *testing.T) {
		raw := `{"authentication":"api_key","revision":3,"key":"fixture-key","expires_at_ms":null}`
		var value providerAuthorization
		if err := json.Unmarshal([]byte(raw), &value); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(value)
		if err != nil || string(encoded) != raw {
			t.Fatalf("authorization encoding changed: %s, %v", encoded, err)
		}
	})
	t.Run("provider_save_keys_remove_and_revision", func(t *testing.T) {
		_, node, storage := schemaFixture(t)
		ctx := t.Context()
		execute := func(command protocol.Command) (protocol.Output, error) {
			var output protocol.Output
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				output, _, err = ExecuteProviderCommand(ctx, db, node, node, protocol.Request{Command: command}, nil)
				return err
			})
			return output, err
		}
		read := func(command protocol.Command) (protocol.Output, error) {
			var output protocol.Output
			err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				output, err = ReadProviderCommand(ctx, db, node, command)
				return err
			})
			return output, err
		}
		provider := providerFixture()
		secret := protocol.NewSecret("fixture-key")
		output, err := execute(protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, ExpectedRevision: 0, Secret: &secret}})
		if err != nil {
			t.Fatal(err)
		}
		saved := output.Data.(protocol.ConversationProvider)
		if saved.Revision != 1 || saved.Credential == nil {
			t.Fatalf("saved provider=%+v", saved)
		}
		encoded, _ := json.Marshal(saved)
		if strings.Contains(string(encoded), secret.Expose()) {
			t.Fatal("public provider contains a secret")
		}
		output, err = read(protocol.Command{Kind: "read_provider_key", Data: &protocol.CommandReadProviderKeyPayload{Provider: provider.ID, ExpectedRevision: 1}})
		if err != nil {
			t.Fatal(err)
		}
		if key, ok := output.Data.(*protocol.Secret); !ok || key.Expose() != secret.Expose() {
			t.Fatal("explicit key read changed")
		}
		updated := protocol.NewSecret("updated-key")
		output, err = execute(protocol.Command{Kind: "put_credential", Data: &protocol.CommandPutCredentialPayload{ID: saved.Credential.ID, Provider: provider.ID, ExpectedRevision: 1, Secret: updated}})
		if err != nil {
			t.Fatal(err)
		}
		credential := output.Data.(protocol.Credential)
		if credential.Revision != 2 || credential.Revoked {
			t.Fatalf("credential=%+v", credential)
		}
		if _, err := execute(protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: saved, ExpectedRevision: 0, Secret: &secret}}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			t.Fatalf("stale save=%v", err)
		}
		if _, err := execute(protocol.Command{Kind: "remove_provider", Data: &protocol.CommandRemoveProviderPayload{Provider: saved.ID, ExpectedRevision: 1}}); err != nil {
			t.Fatal(err)
		}
		output, err = read(protocol.Command{Kind: "list_providers"})
		if err != nil {
			t.Fatal(err)
		}
		if len(output.Data.([]protocol.ConversationProvider)) != 0 {
			t.Fatal("removed provider remains listed")
		}
		output, err = read(protocol.Command{Kind: "list_credentials"})
		if err != nil {
			t.Fatal(err)
		}
		credentials := output.Data.([]protocol.Credential)
		if len(credentials) != 1 || credentials[0].Revision != 2 {
			t.Fatalf("removal lost authorization: %+v", credentials)
		}
		if count := schemaInteger(t, storage.Node, "SELECT count(*) FROM providers"); count != 1 {
			t.Fatal("removal destroyed provider history capture")
		}
	})
	t.Run("provider_network_revision_recheck", func(t *testing.T) {
		_, node, storage := schemaFixture(t)
		ctx := t.Context()
		provider := providerFixture()
		var saved protocol.ConversationProvider
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			output, _, err := ExecuteProviderCommand(ctx, db, node, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: provider, ExpectedRevision: 0}}}, nil)
			if err == nil {
				saved = output.Data.(protocol.ConversationProvider)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		request := protocol.Request{Command: protocol.Command{Kind: "validate_provider", Data: &protocol.CommandValidateProviderPayload{Provider: saved.ID, ExpectedRevision: saved.Revision}}}
		var work *providers.Work
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			work, err = PrepareProviderCommand(ctx, db, node, node, request)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if work == nil || work.Provider == nil || work.Provider.Revision != 1 {
			t.Fatal("discovery did not capture the provider revision")
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := ExecuteProviderCommand(ctx, db, node, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: saved, ExpectedRevision: 1}}}, nil)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		preparedOutput := protocol.Output{Kind: "provider_validation", Data: protocol.ConversationDiscoveryValidation{Provider: saved.ID, Revision: 1}}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := ExecuteProviderCommand(ctx, db, node, node, request, &providers.Prepared{Output: &preparedOutput})
			return err
		})
		if !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			t.Fatalf("discovery overwrote a revised provider: %v", err)
		}
	})
	t.Run("provider_catalog_and_failure_keep_cache", func(t *testing.T) {
		_, node, storage := schemaFixture(t)
		ctx := t.Context()
		download, err := providers.ParseCatalog([]byte(`{"fixture":{"models":{"reasoner":{"id":"reasoner","name":"Reference","limit":{"context":100000,"output":8192},"modalities":{"input":["text"],"output":["text"]},"tool_call":true,"reasoning":false}}}}`))
		if err != nil {
			t.Fatal(err)
		}
		execute := func(prepared *providers.Prepared) (protocol.ConversationCatalogStatus, error) {
			var status protocol.ConversationCatalogStatus
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				output, _, err := ExecuteProviderCommand(ctx, db, node, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, prepared)
				if err == nil {
					status = output.Data.(protocol.ConversationCatalogStatus)
				}
				return err
			})
			return status, err
		}
		status, err := execute(&providers.Prepared{Catalog: download})
		if err != nil {
			t.Fatal(err)
		}
		if status.Revision != 1 || status.Models != 1 || status.Fault != nil {
			t.Fatalf("catalog status=%+v", status)
		}
		failed := &providers.CatalogDownload{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture unavailable"}}
		status, err = execute(&providers.Prepared{Catalog: failed})
		if err != nil {
			t.Fatal(err)
		}
		if status.Revision != 1 || status.Models != 1 || status.Fault == nil {
			t.Fatalf("failed refresh lost prior metadata: %+v", status)
		}
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			page, err := readProviderCatalog(ctx, db, protocol.ConversationCatalogQuery{IDs: []string{"reasoner"}, Limit: 100})
			if err != nil {
				return err
			}
			if len(page.Models) != 1 {
				return errors.New("failed refresh removed cached models")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
}
func providerFixture() protocol.ConversationProvider {
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name = "Fixture"
	provider.Api = protocol.ConversationModelApiChatCompletions
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "http://127.0.0.1:1/v1"
	provider.Enabled = true
	provider.DefaultModel = "fixture"
	model := protocol.DefaultConversationModel()
	model.ID = "fixture"
	model.Context = 200000
	model.Output = 8192
	provider.Models = []protocol.ConversationModel{model}
	return provider
}
func providerFixtureFault(err error, code protocol.ErrorCode) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == code
}
