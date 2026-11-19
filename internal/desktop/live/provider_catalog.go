package live

import (
	"context"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type providerDiscovery struct {
	generation, revision uint64
	endpoint             string
	authentication       protocol.Authentication
	api                  protocol.ConversationModelApi
	models               map[string]protocol.ConversationModel
}

func (r *Runtime) finishDiscovery(provider protocol.ConversationProvider, generation uint64, discovered []protocol.ConversationDiscoveryModel, known map[string]protocol.ConversationModel, draftModels map[string]providerDiscovery, done func([]viewmodel.ChannelModel)) {
	r.async(func() {
		ctx, cancel := context.WithTimeout(r.ctx, 10*time.Second)
		defer cancel()
		ids := make([]string, 0, len(discovered))
		for _, model := range discovered {
			ids = append(ids, model.ID)
		}
		metadata, err := r.readProviderCatalog(ctx, provider, ids)
		r.enqueue(func(c *ui.Context) {
			// An account server owns its model list. The public catalog only
			// enriches its metadata and may lag or be unavailable.
			if err != nil && provider.Authentication == protocol.AuthenticationApiKey {
				key := "provider_catalog_read_failed"
				if faultCode(err) == protocol.ErrorCodeNotConfigured {
					key = "provider_catalog_required"
				}
				r.reportKeys(c, err, key, key, key)
				done(nil)
				return
			}
			var models []viewmodel.ChannelModel
			for _, original := range discovered {
				model := known[original.ID]
				var reference *protocol.ConversationCatalogModel
				if catalog, ok := metadata[original.ID]; ok {
					reference = &catalog
					if original.Context == nil && catalog.Context != nil {
						model.Context = *catalog.Context
					}
					if original.Output == nil && catalog.Output != nil {
						model.Output = *catalog.Output
					}
					if model.DisplayName == nil && catalog.Name != "" {
						model.DisplayName = new(catalog.Name)
					}
				}
				if original.Output == nil {
					model.Output = min(model.Output, model.Context)
				}
				model = providers.CompleteChannelModel(provider, model, reference)
				known[model.ID] = model
				models = append(models, channelModel(model))
			}
			if key := provider.ID.String(); draftModels[key].generation <= generation {
				draftModels[key] = providerDiscovery{generation: generation, revision: provider.Revision,
					endpoint: provider.Endpoint, authentication: provider.Authentication, api: provider.Api, models: known}
			}
			done(models)
		})
	})
}

func (r *Runtime) lookupProviderModel(provider protocol.ConversationProvider, id string, done func(*viewmodel.ChannelModel)) {
	r.async(func() {
		ctx, cancel := context.WithTimeout(r.ctx, 10*time.Second)
		defer cancel()
		models, err := r.readProviderCatalog(ctx, provider, []string{id})
		reference, found := models[id]
		r.enqueue(func(*ui.Context) {
			if err != nil || !found {
				done(nil)
				return
			}
			model := protocol.DefaultConversationModel()
			model.ID, model.Context, model.Output = id, 128000, 16384
			completed := channelModel(providers.CompleteChannelModel(provider, model, &reference))
			done(&completed)
		})
	})
}

func (r *Runtime) readProviderCatalog(ctx context.Context, provider protocol.ConversationProvider, ids []string) (map[string]protocol.ConversationCatalogModel, error) {
	result := map[string]protocol.ConversationCatalogModel{}
	remaining := map[string]bool{}
	for _, id := range ids {
		remaining[id] = true
	}
	channel := &protocol.ConversationCatalogChannel{Endpoint: provider.Endpoint, Authentication: provider.Authentication}
	var revision *uint64
	for range 32 {
		if len(remaining) == 0 {
			return result, nil
		}
		ids := make([]string, 0, len(remaining))
		for id := range remaining {
			ids = append(ids, id)
		}
		slices.Sort(ids)
		ids = ids[:min(100, len(ids))]
		out, err := r.controller.Do(ctx, protocol.Command{Kind: "read_model_catalog", Data: protocol.ConversationCatalogQuery{Channel: channel, IDs: ids, Revision: revision, Limit: 100}})
		if err != nil {
			return result, err
		}
		page, ok := detail[protocol.ConversationCatalogPage](out.Data)
		if !ok || out.Kind != "model_catalog" {
			return result, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Model catalog page expected"}}
		}
		for _, model := range page.Models {
			if remaining[model.ID] {
				result[model.ID] = model
				delete(remaining, model.ID)
			}
		}
		for _, id := range ids {
			if page.Next == nil || id <= *page.Next {
				delete(remaining, id)
			}
		}
		revision = &page.Revision
	}
	return result, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Catalog lookup exceeded its page limit"}}
}
