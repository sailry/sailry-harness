package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"
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
				output, _, err = ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: command}, nil)
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
		if len(credentials) != 1 || credentials[0].ID != saved.Credential.ID || credentials[0].Revision != 1 || credentials[0].Revoked {
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
			output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: provider, ExpectedRevision: 0}}}, nil)
			if err == nil {
				saved = output.Data.(protocol.ConversationProvider)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		request := protocol.Request{Command: protocol.Command{Kind: "discover_models", Data: &protocol.ConversationDiscoverySource{Kind: "saved", Data: protocol.ConversationDiscoverySourceSavedPayload{Provider: saved.ID, ExpectedRevision: saved.Revision}}}}
		var work *providers.Work
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			work, err = PrepareProviderCommand(ctx, db, node, request)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if work == nil || work.Provider == nil || work.Provider.Revision != 1 {
			t.Fatal("discovery did not capture the provider revision")
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: saved, ExpectedRevision: 1}}}, nil)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		preparedOutput := protocol.Output{Kind: "discovered_models", Data: protocol.DefaultConversationDiscoveryCatalog()}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			_, _, err := ExecuteProviderCommand(ctx, db, node, request, &providers.Prepared{Output: &preparedOutput})
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
				output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, prepared)
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
	t.Run("provider_capability_tags_and_media_assignments", func(t *testing.T) {
		_, node, storage := schemaFixture(t)
		ctx := t.Context()
		write := func(command protocol.Command, prepared *providers.Prepared) (protocol.Output, error) {
			var output protocol.Output
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				if command.Kind == "save_media_settings" {
					output, _, err = saveMediaSettings(ctx, db, *command.Data.(*protocol.MediaSettings))
				} else {
					output, _, err = ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: command}, prepared)
				}
				return err
			})
			return output, err
		}
		provider := providerFixture()
		automatic, tagged, speaker := provider.Models[0], provider.Models[0], provider.Models[0]
		automatic.ID = "reasoner"
		tagged.ID, tagged.CustomCapabilities, tagged.Vision, tagged.Tools = "tagged", true, true, false
		tagged.Generates = []protocol.MediaGeneration{protocol.MediaGenerationVideo, protocol.MediaGenerationImage, protocol.MediaGenerationImage}
		speaker.ID = "speaker"
		provider.Models, provider.DefaultModel = []protocol.ConversationModel{automatic, tagged, speaker}, "reasoner"
		output, err := write(protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: provider, ExpectedRevision: 0}}, nil)
		if err != nil {
			t.Fatal(err)
		}
		saved := output.Data.(protocol.ConversationProvider)
		image, video := protocol.MediaGenerationImage, protocol.MediaGenerationVideo
		if got := saved.Models[1]; !got.CustomCapabilities || !got.Vision || got.Tools || !reflect.DeepEqual(got.Generates, []protocol.MediaGeneration{image, video}) {
			t.Fatalf("saved user tags = %+v", got)
		}
		// The catalog disagrees with every user tag and supplies the defaults
		// of the two automatic models.
		download, err := providers.ParseCatalog([]byte(`{"fixture":{"models":{
			"reasoner":{"id":"reasoner","name":"Reference","limit":{"context":100000,"output":8192},"modalities":{"input":["text"],"output":["text","audio"]},"tool_call":false,"reasoning":false},
			"tagged":{"id":"tagged","name":"Tagged","limit":{"context":100000,"output":8192},"modalities":{"input":["text"],"output":["text"]},"tool_call":true,"reasoning":true},
			"speaker":{"id":"speaker","name":"Speaker","limit":{"context":4096,"output":4096},"modalities":{"input":["text"],"output":["audio"]},"tool_call":false,"reasoning":false}}}}`))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := write(protocol.Command{Kind: "refresh_model_catalog"}, &providers.Prepared{Catalog: download}); err != nil {
			t.Fatal(err)
		}
		var listed []protocol.ConversationProvider
		var candidates []protocol.MediaCandidate
		read := func() protocol.MediaSettings {
			var settings protocol.MediaSettings
			if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				if listed, err = providerList(ctx, db); err != nil {
					return err
				}
				if candidates, err = MediaModels(ctx, db); err != nil {
					return err
				}
				settings, err = ReadMediaSettings(ctx, db)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			return settings
		}
		read()
		refreshed := listed[0]
		if got := refreshed.Models[0]; got.Vision || got.Tools || len(got.Generates) != 0 {
			t.Fatalf("automatic tags did not follow the catalog: %+v", got)
		}
		if got := refreshed.Models[1]; !got.CustomCapabilities || !got.Vision || got.Tools || got.Reasoning || !reflect.DeepEqual(got.Generates, []protocol.MediaGeneration{image, video}) {
			t.Fatalf("catalog refresh replaced user tags: %+v", got)
		}
		if got := refreshed.Models[2]; !reflect.DeepEqual(got.Generates, []protocol.MediaGeneration{protocol.MediaGenerationSpeech}) {
			t.Fatalf("audio-only catalog model is not a speech model: %+v", got)
		}
		vision, picture, film, speech := protocol.MediaKindVision, protocol.MediaKindImage, protocol.MediaKindVideo, protocol.MediaKindSpeech
		want := []protocol.MediaCandidate{
			{Provider: provider.ID, ProviderName: provider.Name, Model: "tagged", Kinds: []protocol.MediaKind{vision, picture, film}},
			{Provider: provider.ID, ProviderName: provider.Name, Model: "speaker", Kinds: []protocol.MediaKind{speech}},
		}
		if !reflect.DeepEqual(candidates, want) {
			t.Fatalf("media candidates = %+v", candidates)
		}
		bind := func(revision uint64, bindings map[protocol.MediaKind]*protocol.MediaBinding) (protocol.MediaSettings, error) {
			output, err := write(protocol.Command{Kind: "save_media_settings", Data: &protocol.MediaSettings{Revision: revision, Bindings: bindings}}, nil)
			if err != nil {
				return protocol.MediaSettings{}, err
			}
			return output.Data.(protocol.MediaSettings), nil
		}
		choose := func(name string) *protocol.MediaBinding {
			return &protocol.MediaBinding{Provider: provider.ID, Model: name}
		}
		if _, err := bind(0, map[protocol.MediaKind]*protocol.MediaBinding{speech: choose("tagged")}); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			t.Fatalf("a model without the speech tag was assigned: %v", err)
		}
		if _, err := bind(0, map[protocol.MediaKind]*protocol.MediaBinding{picture: choose("absent")}); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			t.Fatalf("an absent model was assigned: %v", err)
		}
		assigned := map[protocol.MediaKind]*protocol.MediaBinding{vision: choose("tagged"), picture: choose("tagged"), speech: choose("speaker")}
		settings, err := bind(0, assigned)
		if err != nil || settings.Revision != 1 || len(settings.Unavailable) != 0 || !reflect.DeepEqual(settings.Bindings, assigned) {
			t.Fatalf("media settings = %+v, %v", settings, err)
		}
		// The user removes the image tag: the binding stays, is reported and
		// captures no provider, so nothing replaces it.
		refreshed.Models[1].Generates = []protocol.MediaGeneration{video}
		output, err = write(protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: refreshed, ExpectedRevision: refreshed.Revision}}, nil)
		if err != nil {
			t.Fatal(err)
		}
		refreshed = output.Data.(protocol.ConversationProvider)
		if settings = read(); !reflect.DeepEqual(settings.Unavailable, []protocol.MediaKind{picture}) || !reflect.DeepEqual(settings.Bindings, assigned) {
			t.Fatalf("lost tag was not reported: %+v", settings)
		}
		var capture MediaCapture
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error { var err error; capture, err = CaptureMedia(ctx, db); return err }); err != nil {
			t.Fatal(err)
		}
		if len(capture) != 3 || capture[picture].Provider != nil || capture[picture].Model != "tagged" || capture[vision].Provider == nil || capture[vision].Provider.ID != provider.ID || capture[speech].Model != "speaker" {
			t.Fatalf("captured assignments = %+v", capture)
		}
		// Keeping an unavailable binding while changing another purpose saves.
		kept := map[protocol.MediaKind]*protocol.MediaBinding{picture: choose("tagged"), film: choose("tagged")}
		if settings, err = bind(1, kept); err != nil || !reflect.DeepEqual(settings.Unavailable, []protocol.MediaKind{picture}) || !reflect.DeepEqual(settings.Bindings, kept) {
			t.Fatalf("kept binding = %+v, %v", settings, err)
		}
		refreshed.Enabled = false
		if _, err = write(protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: refreshed, ExpectedRevision: refreshed.Revision}}, nil); err != nil {
			t.Fatal(err)
		}
		if settings = read(); !reflect.DeepEqual(settings.Unavailable, []protocol.MediaKind{picture, film}) || len(candidates) != 0 {
			t.Fatalf("disabled provider = %+v, candidates %+v", settings, candidates)
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
