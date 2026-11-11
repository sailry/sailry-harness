package service

import (
	"context"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func valuePosition(record store.KnowledgeStorageRecord) protocol.KnowledgePosition {
	return protocol.KnowledgePosition{Kind: "value", Data: protocol.KnowledgePositionValuePayload{Key: record.Source.Key, Pointer: record.Source.Pointer}}
}

// storage indexes the plugin values of a storage collection. Each value is
// one source whose revision covers the entry, its index data and the text it
// points to, so a pass encodes only the values that changed.
func (service *Service) storage(ctx context.Context, stored store.KnowledgeStored, binding Binding, model string, choice protocol.ModelsLocalChoice, progress protocol.KnowledgeProgress, report func(protocol.KnowledgeProgress) error) error {
	index := service.index
	active, working := false, func() {}
	defer func() { working() }()
	clock := service.options.Clock
	last := clock.Now()
	var locators []string
	var after *string
	for {
		page, err := service.options.Sources(ctx, stored, after)
		if err != nil {
			return err
		}
		for _, record := range page.Records {
			if err := indexingCancelled(ctx); err != nil {
				return err
			}
			document := Document{
				Locator: record.Key, Revision: record.Identity(), Embedded: model != "",
				Tags: append(slices.Clone(stored.Collection.Definition.Tags), record.Tags...),
			}
			progress.Sources++
			locators = append(locators, record.Key)
			unchanged, err := index.Unchanged(ctx, binding, document)
			if err != nil {
				return err
			}
			if !unchanged {
				if !active {
					active = true
					if service.options.Working != nil {
						working = service.options.Working()
					}
					if err := report(progress); err != nil {
						return err
					}
				}
				parts, err := service.parts(ctx, model, choice, source.Part{Title: record.Title, Text: record.Body, Position: valuePosition(record)})
				if err != nil {
					return err
				}
				if err := indexingCancelled(ctx); err != nil {
					return err
				}
				current, err := service.options.SourceCurrent(ctx, stored, record)
				if err != nil {
					return err
				}
				if !current {
					return newFault(protocol.ErrorCodeRevisionConflict, "knowledge storage source changed")
				}
				if err := index.Replace(ctx, binding, document, parts); err != nil {
					return err
				}
			}
			progress.Completed++
			if active && clock.Since(last) >= 500*time.Millisecond {
				if _, progress.Chunks, err = index.Counts(ctx, binding); err != nil {
					return err
				}
				if err := report(progress); err != nil {
					return err
				}
				last = clock.Now()
			}
		}
		if after = page.After; after == nil {
			break
		}
	}
	if err := indexingCancelled(ctx); err != nil {
		return err
	}
	if err := index.Prune(ctx, binding, locators, nil); err != nil {
		return err
	}
	var err error
	if progress.Sources, progress.Chunks, err = index.Counts(ctx, binding); err != nil {
		return err
	}
	progress.Completed = progress.Sources
	progress.Status = protocol.KnowledgeStatusReady
	return report(progress)
}

// value rereads the plugin value a citation names. The cited text is the
// whole value text.
func (service *Service) value(ctx context.Context, stored store.KnowledgeStored, citation protocol.KnowledgeCitation) (string, error) {
	record, err := service.options.Source(ctx, stored, citation.Path)
	if err != nil {
		return "", err
	}
	if record == nil || record.Identity() != citation.Revision || encoded(citation.Position) != encoded(valuePosition(*record)) {
		return "", changedSource()
	}
	current, err := service.options.SourceCurrent(ctx, stored, *record)
	if err != nil {
		return "", err
	}
	if !current {
		return "", changedSource()
	}
	return record.Body, nil
}

// SearchStorage answers a plugin's search of its own values. Meaning is
// ranked by the automatic collections of the package and fused with the
// keyword ranking; every returned value is read as it is now.
func (service *Service) SearchStorage(ctx context.Context, request protocol.Request) (protocol.PluginStorageSearchPage, error) {
	none := protocol.DefaultPluginStorageSearchPage()
	query, found := request.Command.Data.(*protocol.PluginStorageSearch)
	if !found {
		return none, invalid("plugin storage search expected")
	}
	plan, err := service.options.Plan(ctx, request)
	if err != nil {
		return none, err
	}
	var hits []protocol.KnowledgeHit
	indexing, available := false, false
	if query.Query != nil && strings.TrimSpace(*query.Query) != "" && query.Mode != protocol.KnowledgeModeKeyword && len(plan.Collections) != 0 {
		retrieval := protocol.DefaultKnowledgeQuery()
		retrieval.Query, retrieval.Mode, retrieval.Tags, retrieval.Limit = *query.Query, protocol.KnowledgeModeSemantic, query.All, knowledge.DefaultLimit
		for _, stored := range plan.Collections {
			retrieval.Collections = append(retrieval.Collections, stored.Collection.ID)
		}
		prepared, err := service.prepare(ctx, request, retrieval)
		if err != nil {
			return none, err
		}
		indexing, available = prepared.indexing, len(prepared.embeddings) != 0
		if hits, err = service.index.Candidates(ctx, retrieval, prepared.bindings, prepared.embeddings, query.Any); err != nil {
			return none, err
		}
	}
	for _, stored := range plan.Collections {
		status := stored.Collection.Progress.Status
		indexing = indexing || status == protocol.KnowledgeStatusPending || status == protocol.KnowledgeStatusIndexing
	}
	page, stale, err := service.options.Materialize(ctx, request, plan, selectValues(*query, plan, hits, indexing, available))
	if err != nil {
		return none, err
	}
	if stale {
		service.Refresh()
	}
	return page, nil
}

// selectValues fuses the keyword ranking of a plan with its semantic hits.
// A semantic search keeps keyword matches only while meaning cannot answer.
func selectValues(query protocol.PluginStorageSearch, plan store.SemanticPlan, hits []protocol.KnowledgeHit, indexing, available bool) store.SemanticSelection {
	selection := store.SemanticSelection{Indexing: indexing}
	var semantic, keyword []string
	for _, hit := range hits {
		key := hit.Citation.Path
		if plan.Eligible[key] && !slices.Contains(semantic, key) {
			semantic = append(semantic, key)
			selection.Stamps = append(selection.Stamps, store.SemanticStamp{Collection: hit.Citation.Collection, Key: key, Revision: hit.Citation.Revision})
		}
	}
	for _, rank := range plan.Keyword {
		if query.Mode != protocol.KnowledgeModeSemantic || indexing || !available || len(semantic) == 0 || !plan.Eligible[rank.Key] {
			keyword = append(keyword, rank.Key)
		}
	}
	switch {
	case len(semantic) == 0:
		selection.Mode, selection.Keys = protocol.KnowledgeModeKeyword, keyword
	case len(keyword) == 0:
		selection.Mode, selection.Keys = protocol.KnowledgeModeSemantic, semantic
	default:
		selection.Mode = protocol.KnowledgeModeHybrid
		for _, ranked := range knowledge.Fuse(keyword, semantic) {
			selection.Keys = append(selection.Keys, ranked.Key)
		}
	}
	return selection
}
