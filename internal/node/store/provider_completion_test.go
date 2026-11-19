package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderInputsFollowCatalogWithoutReplacingOverrides(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Api, provider.Endpoint = protocol.ConversationModelApiGemini, "https://generativelanguage.googleapis.com/v1beta"
	provider.DefaultModel = "automatic"
	auto := provider.Models[0]
	auto.ID, auto.CompactionTokens = provider.DefaultModel, new(uint32(6144))
	auto.CustomEfforts, auto.Efforts = true, []protocol.Effort{{Kind: "low"}}
	auto.DefaultEffort = auto.Efforts[0]
	manual := auto
	manual.ID, manual.CustomCapabilities, manual.Tools, manual.Reasoning = "manual", true, true, true
	manual.Inputs = []string{"text", "audio"}
	manual.Generates = []protocol.MediaGeneration{protocol.MediaGenerationVideo}
	clear := manual
	clear.ID, clear.Tools, clear.Inputs, clear.Generates = "clear", false, []string{}, []protocol.MediaGeneration{}
	clear.Efforts, clear.DefaultEffort = []protocol.Effort{}, protocol.Effort{Kind: "default"}
	provider.Models = []protocol.ConversationModel{auto, manual, clear}

	refresh := func(automatic string) []protocol.Event {
		t.Helper()
		models := `"manual":{"id":"manual","modalities":{"input":["text","image","pdf"]},"tool_call":false,"reasoning":false},
			"clear":{"id":"clear","modalities":{"input":["text","image","pdf"]},"tool_call":true,"reasoning":true,
				"reasoning_options":[{"type":"effort","values":["high"]}]}`
		if automatic != "" {
			models += `,"automatic":{"id":"automatic","modalities":{"input":` + automatic + `,"output":["text"]},
				"tool_call":false,"reasoning":true,"reasoning_options":[{"type":"effort","values":["high"]}],"limit":{"context":999999,"output":99999}}`
		}
		download, err := providers.ParseCatalog([]byte(`{"google":{"models":{` + models + `}},
			"other":{"models":{"automatic":{"id":"automatic","modalities":{"input":["text","image","pdf"]}}}}}`))
		if err != nil {
			t.Fatal(err)
		}
		var events []protocol.Event
		err = storage.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			_, events, err = ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, &providers.Prepared{Catalog: download})
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		return events
	}

	refresh(`["text","image","video","audio","pdf"]`)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		// A configured list is a v1 modality list, not a list of MIME types.
		invalid := provider
		invalid.Models = []protocol.ConversationModel{auto}
		invalid.Models[0].Inputs = []string{"application/pdf"}
		command := protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: invalid}}
		if _, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: command}, nil); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return fmt.Errorf("unsupported input was not refused: %v", err)
		}
		command.Data = protocol.CommandPutProviderPayload{Provider: provider}
		_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: command}, nil)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	assertSaved := func(revision uint64, inputs []string, vision bool) {
		t.Helper()
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			stored, err := readProviderConfiguration(ctx, db, provider.ID)
			if err != nil {
				return err
			}
			if stored == nil || stored.Provider == nil || stored.Provider.Revision != revision || len(stored.Provider.Models) != 3 {
				return fmt.Errorf("expected three saved models at revision %d, got %+v", revision, stored)
			}
			models := stored.Provider.Models
			var outputs []string
			if inputs != nil {
				outputs = []string{"text"}
			}
			if model := models[0]; model.Vision != vision || !reflect.DeepEqual(model.Inputs, inputs) ||
				!reflect.DeepEqual(model.Outputs, outputs) ||
				model.Context != auto.Context || model.Output != auto.Output || !reflect.DeepEqual(model.CompactionTokens, auto.CompactionTokens) ||
				!model.CustomEfforts || !reflect.DeepEqual(model.Efforts, auto.Efforts) || !reflect.DeepEqual(model.DefaultEffort, auto.DefaultEffort) {
				return fmt.Errorf("automatic model should follow inputs %#v without changing its limits or efforts: %+v", inputs, model)
			}
			if !reflect.DeepEqual(models[1], manual) || !reflect.DeepEqual(models[2], clear) {
				return fmt.Errorf("manual tags or explicit clear changed: %+v", models[1:])
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	assertSaved(1, []string{"text", "image", "video", "audio", "pdf"}, true)
	for _, step := range []struct {
		catalog string
		inputs  []string
		vision  bool
	}{
		{catalog: `["text","pdf"]`, inputs: []string{"text", "pdf"}},
		{catalog: `[]`, inputs: []string{}},
		{vision: true}, // Same ID remains in another namespace only; no previous input list survives.
	} {
		events := refresh(step.catalog)
		if len(events) != 2 || events[0].Kind != "provider_changed" || events[1].Kind != "model_catalog_changed" {
			t.Fatalf("catalog update did not publish the changed provider: %+v", events)
		}
		revision := events[0].Data.(protocol.ConversationProvider).Revision
		assertSaved(revision, step.inputs, step.vision)
		if events := refresh(step.catalog); len(events) != 1 || events[0].Kind != "model_catalog_changed" {
			t.Fatalf("identical metadata republished providers: %+v", events)
		}
		assertSaved(revision, step.inputs, step.vision)
	}
}
