package live

import (
	"context"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) finishDiscovery(provider protocol.ConversationProvider, discovered []protocol.ConversationDiscoveryModel, known map[string]protocol.ConversationModel, done func([]viewmodel.ChannelModel)) {
	r.async(func() {
		ctx, cancel := context.WithTimeout(r.ctx, 10*time.Second)
		defer cancel()
		metadata, err := r.readProviderCatalog(ctx, provider.Api, discovered)
		r.enqueue(func(c *ui.Context) {
			if err != nil && faultCode(err) != protocol.ErrorCodeNotConfigured {
				r.reportKeys(c, err, "provider_catalog_read_failed", "provider_catalog_read_failed", "provider_catalog_read_failed")
			}
			var models []viewmodel.ChannelModel
			for _, original := range discovered {
				model := known[original.ID]
				if catalog, ok := metadata[original.ID]; ok {
					if original.Context == nil && catalog.Context != nil {
						model.Context = *catalog.Context
					}
					if original.Output == nil && catalog.Output != nil {
						model.Output = *catalog.Output
					}
					if model.DisplayName == nil && catalog.Name != "" {
						model.DisplayName = new(catalog.Name)
					}
					caps := original.Capabilities
					if caps == nil || caps.Tools == nil {
						if catalog.Tools != nil {
							model.Tools = *catalog.Tools
						}
					}
					if caps == nil || caps.Vision == nil {
						model.Vision = slices.Contains(catalog.Inputs, "image")
					}
					if caps == nil || caps.Reasoning == nil {
						if catalog.Reasoning != nil {
							model.Reasoning = *catalog.Reasoning
						}
					}
					if caps == nil || caps.Efforts == nil {
						for _, option := range catalog.Options {
							if option.Kind == "effort" {
								if efforts, ok := detail[protocol.ConversationCatalogReasoningEffortPayload](option.Data); ok {
									model.Efforts = nil
									for _, value := range efforts.Values {
										if value != nil {
											model.Efforts = append(model.Efforts, parseEffort(*value))
										} else {
											model.Efforts = append(model.Efforts, protocol.Effort{Kind: "default"})
										}
									}
								}
							}
						}
					}
					if len(model.Efforts) > 0 && (caps == nil || caps.DefaultEffort == nil) {
						model.DefaultEffort = model.Efforts[min(1, len(model.Efforts)-1)]
					}
				}
				known[model.ID] = model
				models = append(models, channelModel(model))
			}
			r.providerModels[provider.ID.String()] = known
			done(models)
		})
	})
}

func (r *Runtime) readProviderCatalog(ctx context.Context, api protocol.ConversationModelApi, models []protocol.ConversationDiscoveryModel) (map[string]protocol.ConversationCatalogModel, error) {
	result := map[string]protocol.ConversationCatalogModel{}
	remaining := map[string]bool{}
	for _, model := range models {
		remaining[model.ID] = true
	}
	var provider *string
	switch api {
	case "responses", "chat_completions":
		provider = new("openai")
	case "anthropic":
		provider = new("anthropic")
	case "gemini":
		provider = new("google")
	case "deepseek":
		provider = new("deepseek")
	case "open_code_go":
		provider = new("opencode-go")
	case "open_code_zen":
		provider = new("opencode")
	}
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
		out, err := r.controller.Do(ctx, protocol.Command{Kind: "read_model_catalog", Data: protocol.ConversationCatalogQuery{Provider: provider, IDs: ids, Revision: revision, Limit: 100}})
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
