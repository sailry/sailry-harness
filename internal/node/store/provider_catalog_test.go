package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBundledProviderCatalogSurvivesFailedRefresh(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	read := func() protocol.ConversationCatalogStatus {
		t.Helper()
		var status protocol.ConversationCatalogStatus
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			status, err = ReadProviderCatalogStatus(ctx, db)
			if err != nil {
				return err
			}
			page, err := readProviderCatalog(ctx, db, protocol.ConversationCatalogQuery{Provider: new("openai"), IDs: []string{"gpt-6.1-sol"}, Limit: 1})
			if err != nil {
				return err
			}
			if len(page.Models) != 1 || page.Models[0].Reasoning == nil || !*page.Models[0].Reasoning {
				return fmt.Errorf("bundled model metadata is unavailable: %+v", page)
			}
			efforts := providers.CatalogEfforts(page.Models[0], protocol.ConversationModelApiResponses, 8192)
			if !slices.ContainsFunc(efforts, func(effort protocol.Effort) bool { return effort.Kind == "max" }) {
				return fmt.Errorf("bundled reasoning choices are unavailable: %+v", efforts)
			}
			page, err = readProviderCatalog(ctx, db, protocol.ConversationCatalogQuery{
				Channel: &protocol.ConversationCatalogChannel{Endpoint: "https://custom.invalid/v1", Authentication: protocol.AuthenticationApiKey},
				IDs:     []string{"gpt-6.1-sol"},
				Limit:   1,
			})
			if err != nil {
				return err
			}
			if len(page.Models) != 1 || !slices.Contains(page.Models[0].Inputs, "image") {
				return fmt.Errorf("bundled channel lookup did not recover the GPT-6.1 Sol image input")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		return status
	}
	initial := read()
	if !initial.Bundled || initial.Revision != 0 || initial.UpdatedAtMs != nil || initial.Providers == 0 || initial.Models == 0 || initial.Bytes == 0 || initial.Fault != nil {
		t.Fatalf("bundled catalog status = %+v", initial)
	}
	if count := schemaInteger(t, storage.Node, "SELECT count(*) FROM model_catalog"); count != 0 {
		t.Fatalf("reading the bundle wrote %d database rows", count)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, &providers.Prepared{
			Catalog: &providers.CatalogDownload{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture offline"}},
		})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	failed := read()
	if !failed.Bundled || failed.Revision != 0 || failed.UpdatedAtMs != nil || failed.Fault == nil || failed.Models != initial.Models || failed.Bytes != initial.Bytes {
		t.Fatalf("failed refresh lost bundled metadata: %+v", failed)
	}
}

func TestProviderCatalogSourcesSharePaginationAndMerging(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	query := protocol.ConversationCatalogQuery{Provider: new("openai"), Limit: 1}
	var first, second protocol.ConversationCatalogPage
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		first, err = readProviderCatalog(ctx, db, query)
		if err != nil {
			return err
		}
		if len(first.Models) != 1 || first.Next == nil || first.Revision != 0 {
			return fmt.Errorf("invalid bundled first page: %+v", first)
		}
		query.Revision, query.After = &first.Revision, first.Next
		second, err = readProviderCatalog(ctx, db, query)
		if err != nil {
			return err
		}
		if len(second.Models) != 1 || second.Models[0].ID <= first.Models[0].ID {
			return fmt.Errorf("bundled pagination repeated a model: %+v", second)
		}
		query.Revision = new(uint64(1))
		if _, err := readProviderCatalog(ctx, db, query); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("bundled stale revision was accepted: %v", err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	lookup := protocol.ConversationCatalogQuery{IDs: []string{"gpt-6.1-sol", "gpt-6.1-sol"}, Limit: 1}
	var before protocol.ConversationCatalogPage
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		before, err = readProviderCatalog(ctx, db, lookup)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	bundle, err := providers.BundledCatalog()
	if err != nil {
		t.Fatal(err)
	}
	download := &providers.CatalogDownload{}
	sources := map[string]bool{}
	for _, entry := range bundle.Entries {
		if entry.ID == lookup.IDs[0] {
			download.Entries = append(download.Entries, entry)
			sources[entry.Provider] = true
			download.Bytes += uint64(len(entry.Body))
		}
	}
	download.Providers = uint32(len(sources))
	if len(sources) < 2 {
		t.Fatal("bundled merge fixture requires multiple sources")
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		out, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, &providers.Prepared{Catalog: download})
		if err != nil {
			return err
		}
		status := out.Data.(protocol.ConversationCatalogStatus)
		if status.Bundled || status.Revision != 1 || status.UpdatedAtMs == nil || status.Models != uint32(len(download.Entries)) {
			return fmt.Errorf("downloaded status still exposes bundled data: %+v", status)
		}
		after, err := readProviderCatalog(ctx, db, lookup)
		if err != nil {
			return err
		}
		if len(after.Models) != 1 || !reflect.DeepEqual(after.Models, before.Models) {
			return fmt.Errorf("source selection changed conservative merging: before=%+v after=%+v", before.Models, after.Models)
		}
		query.Revision = &first.Revision
		if _, err := readProviderCatalog(ctx, db, query); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("download accepted a bundled pagination cursor: %v", err)
		}
		missing, err := readProviderCatalog(ctx, db, protocol.ConversationCatalogQuery{Provider: new("openai"), IDs: []string{first.Models[0].ID}, Limit: 1})
		if err != nil {
			return err
		}
		if len(missing.Models) != 0 {
			return fmt.Errorf("downloaded catalog was mixed with older bundled models: %+v", missing)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestSaveProviderUsesBundledReasoningAndPreservesCustomChoices(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	provider := providerFixture()
	provider.Api, provider.Endpoint = protocol.ConversationModelApiResponses, "https://api.openai.com/v1"
	provider.DefaultModel, provider.Models[0].ID = "gpt-6.1-sol", "gpt-6.1-sol"
	custom := provider.Models[0]
	custom.ID, custom.CustomCapabilities, custom.CustomEfforts = "gpt-6-sol", true, true
	custom.Vision, custom.Tools, custom.Reasoning = false, false, true
	custom.Generates = []protocol.MediaGeneration{protocol.MediaGenerationImage}
	custom.Efforts = []protocol.Effort{{Kind: "high"}}
	custom.DefaultEffort = custom.Efforts[0]
	provider.Models = append(provider.Models, custom)
	custom.Outputs = []string{"text"} // Read-only catalog metadata accompanies the manual choices.
	var saved protocol.ConversationProvider
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		out, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: protocol.CommandPutProviderPayload{Provider: provider}}}, nil)
		if err != nil {
			return err
		}
		saved = out.Data.(protocol.ConversationProvider)
		model := saved.Models[0]
		if !model.Reasoning || !slices.ContainsFunc(model.Efforts, func(effort protocol.Effort) bool { return effort.Kind == "max" }) {
			return fmt.Errorf("save did not complete bundled reasoning: %+v", model)
		}
		if model.Context != provider.Models[0].Context || model.Output != provider.Models[0].Output {
			return fmt.Errorf("completion replaced configured limits: %+v", model)
		}
		if !reflect.DeepEqual(saved.Models[1], custom) {
			return fmt.Errorf("completion replaced custom choices: %+v", saved.Models[1])
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "refresh_model_catalog"}}, &providers.Prepared{
			Catalog: &providers.CatalogDownload{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture offline"}},
		})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		listed, err := providerList(ctx, db)
		if err != nil {
			return err
		}
		if len(listed) != 1 || !reflect.DeepEqual(listed[0], saved) {
			return fmt.Errorf("failed catalog refresh changed configured models: %+v", listed)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

}

func TestProviderVisionUsesChannelOrCanonicalModel(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	const modelID = "fixture-vision"
	download, err := providers.ParseCatalog([]byte(`{
		"openai":{"models":{
			"fixture-vision":{"id":"fixture-vision","canonical_model_id":"openai/fixture-vision","modalities":{"input":["text","image","video","audio","pdf"]}},
			"fixture-empty":{"id":"fixture-empty","modalities":{"input":[]}}
		}},
		"deepseek":{"models":{"fixture-vision":{"id":"fixture-vision","canonical_model_id":"openai/fixture-vision","modalities":{"input":["text"]}}}},
		"another":{"models":{"fixture-ambiguous":{"id":"fixture-ambiguous","canonical_model_id":"other/fixture-ambiguous","modalities":{"input":["text"]}}}},
		"other":{"models":{
			"fixture-ambiguous":{"id":"fixture-ambiguous","canonical_model_id":"openai/fixture-vision","modalities":{"input":["text"]}},
			"fixture-bare":{"id":"fixture-bare","modalities":{"input":["text","video","audio","pdf"]}}
		}}
	}`))
	if err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := applyProviderCatalog(ctx, db, &providers.Prepared{Catalog: download})
		return err
	}); err != nil {
		t.Fatal(err)
	}

	all := []string{"text", "image", "video", "audio", "pdf"}
	text := []string{"text"}
	for _, row := range []struct {
		name, endpoint, model         string
		auth                          protocol.Authentication
		manual, nativeDeny, oldVision bool
		vision                        bool
		catalog, inputs               []string
	}{
		{name: "canonical vision only on an unlisted gateway", endpoint: "https://custom.invalid/v1", model: modelID, catalog: []string{"text", "image"}, vision: true},
		{name: "explicit text-only channel", endpoint: "https://api.deepseek.com/v1", model: modelID, catalog: text, inputs: text},
		{name: "authenticated official namespace", endpoint: "https://chatgpt.com/backend-api/codex", auth: protocol.AuthenticationChatGptOAuth, model: modelID, oldVision: true, vision: true, catalog: all, inputs: all},
		{name: "native capability overrides only image", endpoint: "https://api.openai.com/v1", model: modelID, nativeDeny: true, catalog: all, inputs: []string{"text", "video", "audio", "pdf"}},
		{name: "manual legacy capability overrides catalog", endpoint: "https://api.openai.com/v1", model: modelID, manual: true, catalog: all},
		{name: "conflicting model identities", endpoint: "https://custom.invalid/v1", model: "fixture-ambiguous", catalog: text},
		{name: "bare ID on an unlisted gateway", endpoint: "https://custom.invalid/v1", model: "fixture-bare", catalog: text},
		{name: "explicit empty on an exact channel", endpoint: "https://api.openai.com/v1", model: "fixture-empty", catalog: []string{}, inputs: []string{}},
	} {
		t.Run(row.name, func(t *testing.T) {
			provider := providerFixture()
			provider.Endpoint, provider.DefaultModel = row.endpoint, row.model
			provider.Revision = 7
			if row.auth != "" {
				provider.Authentication = row.auth
			}
			entry := &provider.Models[0]
			entry.ID, entry.Vision, entry.CustomCapabilities = row.model, row.oldVision, row.manual
			if row.nativeDeny {
				entry.Capabilities = &protocol.ConversationDiscoveryCapabilities{Vision: new(false)}
			}
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				return saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider})
			}); err != nil {
				t.Fatal(err)
			}
			err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				page, err := readProviderCatalog(ctx, db, protocol.ConversationCatalogQuery{
					Channel: &protocol.ConversationCatalogChannel{Endpoint: provider.Endpoint, Authentication: provider.Authentication},
					IDs:     []string{row.model},
					Limit:   1,
				})
				if err != nil {
					return err
				}
				if len(page.Models) != 1 || !reflect.DeepEqual(page.Models[0].Inputs, row.catalog) {
					return fmt.Errorf("channel catalog lookup = %+v, want inputs %#v", page, row.catalog)
				}
				captured, err := captureTurnProvider(ctx, db, provider, row.model)
				if err != nil {
					return err
				}
				if model := captured.Models[0]; model.Vision != row.vision || !reflect.DeepEqual(model.Inputs, row.inputs) {
					return fmt.Errorf("capture did not resolve inputs %#v and vision %v: %+v", row.inputs, row.vision, model)
				}
				snapshot, _, err := ReadProviderSnapshot(ctx, db, node)
				if err != nil {
					return err
				}
				listed, err := providerList(ctx, db)
				if err != nil {
					return err
				}
				for _, values := range [][]protocol.ConversationProvider{snapshot, listed} {
					index := slices.IndexFunc(values, func(value protocol.ConversationProvider) bool { return value.ID == provider.ID })
					if index < 0 || values[index].Models[0].Vision != row.vision || !reflect.DeepEqual(values[index].Models[0].Inputs, row.inputs) || values[index].Revision != provider.Revision {
						return fmt.Errorf("provider projection did not resolve inputs %#v and vision %v at revision %d", row.inputs, row.vision, provider.Revision)
					}
				}
				saved, err := readProviderConfiguration(ctx, db, provider.ID)
				if err != nil {
					return err
				}
				if saved == nil || saved.Provider.Models[0].Inputs != nil || saved.Provider.Models[0].Vision != row.oldVision || saved.Provider.Revision != provider.Revision {
					return fmt.Errorf("resolving legacy model inputs changed the stored configuration")
				}
				if err = fillProviderMetadata(ctx, db, &provider); err != nil {
					return err
				}
				if model := provider.Models[0]; model.Vision != row.vision || !reflect.DeepEqual(model.Inputs, row.inputs) {
					return fmt.Errorf("completion did not resolve inputs %#v and vision %v: %+v", row.inputs, row.vision, model)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
