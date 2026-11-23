package memorynode

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var _ moduleapi.CommandHandler = module{}

func (module) HandlesCommand(kind string) bool {
	switch kind {
	case "read_memory_catalog", "read_memory_entry", "save_memory_entry", "delete_memory_entry", "save_memory_settings":
		return true
	}
	return false
}

func (module) ExecuteCommand(ctx context.Context, host moduleapi.Host, _ protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	client := storageClient{host: host}
	switch request.Command.Kind {
	case "read_memory_catalog":
		input, ok := request.Command.Data.(*protocol.CommandReadMemoryCatalogPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory catalog request")
		}
		catalog, err := client.readCatalogCommand(ctx, *input)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		return protocol.Output{Kind: "memory_catalog", Data: catalog}, nil, nil
	case "read_memory_entry":
		input, ok := request.Command.Data.(*protocol.CommandReadMemoryEntryPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory read request")
		}
		entry, err := client.readMemory(ctx, input.ID, nil)
		if err != nil {
			return protocol.Output{}, nil, mapMemoryError(err)
		}
		return protocol.Output{Kind: "memory_entry", Data: toProtocolEntry(entry)}, nil, nil
	case "save_memory_entry":
		input, ok := request.Command.Data.(*protocol.CommandSaveMemoryEntryPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory save request")
		}
		entry, catalogRev, err := client.saveEntryCommand(ctx, *input)
		if err != nil {
			return protocol.Output{}, nil, mapMemoryError(err)
		}
		return protocol.Output{Kind: "memory_entry", Data: entry},
			[]protocol.Event{{Kind: "memory_changed", Data: protocol.EventMemoryChangedPayload{Revision: catalogRev}}}, nil
	case "delete_memory_entry":
		input, ok := request.Command.Data.(*protocol.CommandDeleteMemoryEntryPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory delete request")
		}
		id, catalogRev, err := client.deleteEntryCommand(ctx, *input)
		if err != nil {
			return protocol.Output{}, nil, mapMemoryError(err)
		}
		return protocol.Output{Kind: "memory_deleted", Data: protocol.OutputMemoryDeletedPayload{ID: id}},
			[]protocol.Event{{Kind: "memory_changed", Data: protocol.EventMemoryChangedPayload{Revision: catalogRev}}}, nil
	case "save_memory_settings":
		input, ok := request.Command.Data.(*protocol.CommandSaveMemorySettingsPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory settings request")
		}
		saved, catalogRev, err := client.saveSettingsCommand(ctx, *input)
		if err != nil {
			return protocol.Output{}, nil, mapMemoryError(err)
		}
		return protocol.Output{Kind: "memory_settings", Data: saved},
			[]protocol.Event{{Kind: "memory_changed", Data: protocol.EventMemoryChangedPayload{Revision: catalogRev}}}, nil
	default:
		return protocol.Output{}, nil, failFault(protocol.ErrorCodeNotConfigured, "unsupported memory command")
	}
}

func mapMemoryError(err error) error {
	if err == nil {
		return nil
	}
	var rc recordChangedError
	if errors.As(err, &rc) {
		return failFault(protocol.ErrorCodeRevisionConflict, "memory revision changed")
	}
	var eq equivalentMemoryError
	if errors.As(err, &eq) {
		return failFault(protocol.ErrorCodeConflict, "equivalent memory already exists")
	}
	if isSnapshotChanged(err) {
		return failFault(protocol.ErrorCodeRevisionConflict, "memory catalog changed")
	}
	return err
}

func toProtocolSettings(s memorySettings) protocol.MemorySettings {
	return protocol.MemorySettings{
		Revision:        s.Revision,
		Enabled:         s.Enabled,
		AutoWrite:       s.AutoWrite,
		ContextBytes:    uint32(s.ContextBytes),
		ReviewAfterDays: uint32(s.ReviewAfterDays),
	}
}

func toProtocolSummary(s memorySummary) protocol.MemorySummary {
	var project *string
	if s.Project != nil {
		p := *s.Project
		project = &p
	}
	return protocol.MemorySummary{
		ID:          s.ID,
		Project:     project,
		Title:       s.Title,
		Kind:        s.Kind,
		Archived:    s.Archived,
		Revision:    uint64(s.Revision),
		UpdatedAtMs: s.UpdatedAtMs,
	}
}

func toProtocolEntry(e memoryEntry) protocol.MemoryEntry {
	return protocol.MemoryEntry{
		Summary: toProtocolSummary(e.Summary),
		Body:    e.Body,
	}
}

func (s storageClient) browseMemories(ctx context.Context, project *string, allProjects, archived bool, query string) ([]memorySummary, error) {
	trimmed := strings.TrimSpace(query)
	stateTag := "state:active"
	if archived {
		stateTag = "state:archived"
	}
	all := []string{stateTag}
	var anyTags []string
	if !allProjects {
		anyTags = []string{scopeTag(project)}
	}
	if uuidRE.MatchString(trimmed) {
		rec, err := s.get(ctx, entryKey(trimmed))
		if err != nil {
			return nil, err
		}
		if !rec.Present {
			return nil, nil
		}
		meta, err := parseStoredEntry(rec)
		if err != nil {
			return nil, err
		}
		if meta.Summary.Archived == archived && (allProjects || sameProject(meta.Summary.Project, project)) {
			return []memorySummary{meta.Summary}, nil
		}
		return nil, nil
	}
	toks := queryTerms(query)
	if trimmed != "" && len(toks) == 0 {
		return nil, nil
	}
	q := protocol.PluginStorageSearch{
		Terms:   toks,
		Mode:    protocol.KnowledgeModeKeyword,
		Weights: [2]uint16{3, 1},
		All:     all,
		Any:     anyTags,
	}
	if trimmed != "" {
		q.Query = &query
		q.Mode = protocol.KnowledgeModeHybrid
	}
	recs, err := s.indexed(ctx, q, 0)
	if err != nil {
		return nil, err
	}
	out := make([]memorySummary, 0, len(recs))
	for _, rec := range recs {
		meta, err := parseStoredEntry(rec)
		if err != nil {
			return nil, err
		}
		out = append(out, meta.Summary)
	}
	return out, nil
}

func (s storageClient) reviewCatalog(ctx context.Context, cfg memorySettings, project *string, allProjects bool) ([]reviewCandidate, error) {
	return stableRead(func() ([]reviewCandidate, error) {
		before, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		var anyTags []string
		if !allProjects {
			anyTags = []string{scopeTag(project)}
		}
		recs, err := s.indexed(ctx, protocol.PluginStorageSearch{
			Terms: []string{}, All: []string{}, Any: anyTags, Mode: protocol.KnowledgeModeKeyword, Weights: [2]uint16{3, 1},
		}, 0)
		if err != nil {
			return nil, err
		}
		after, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		if before.record.Revision != after.record.Revision {
			return nil, snapshotChangedError{msg: "memory catalog changed"}
		}
		entriesByID := make(map[string]storedEntry, len(recs))
		var selected []memorySummary
		for _, rec := range recs {
			meta, err := parseStoredEntry(rec)
			if err != nil {
				return nil, err
			}
			entriesByID[meta.Summary.ID] = meta
			if !meta.Summary.Archived && (allProjects || sameProject(meta.Summary.Project, project)) {
				selected = append(selected, meta.Summary)
			}
		}
		slices.SortFunc(selected, compareSummaryOrder)
		nowMs, err := s.clock(ctx)
		if err != nil {
			return nil, err
		}
		pages := map[string]bodyPageRecord{}
		fingerprints := make([][]string, len(selected))
		for i, sum := range selected {
			meta := entriesByID[sum.ID]
			src, ok := pages[meta.BodyPage]
			if !ok {
				src, err = s.page(ctx, meta.BodyPage)
				if err != nil {
					return nil, err
				}
				pages[meta.BodyPage] = src
			}
			if err := s.unchanged(ctx, after.record.Revision); err != nil {
				return nil, err
			}
			full, err := resolveBody(meta, src)
			if err != nil {
				return nil, err
			}
			if full.Summary.ID != sum.ID || full.Summary.Revision != sum.Revision || !sameProject(full.Summary.Project, sum.Project) || full.Summary.Archived != sum.Archived {
				return nil, failFault(protocol.ErrorCodeRevisionConflict, "memory revision changed during review")
			}
			fingerprints[i] = uniqueSortedTerms(full.Body)
		}
		if err := s.unchanged(ctx, after.record.Revision); err != nil {
			return nil, err
		}
		staleThreshold := int64(cfg.ReviewAfterDays) * 86_400_000
		var candidates []reviewCandidate
		for i, sum := range selected {
			var dupOf *string
			for j := 0; j < i; j++ {
				if sameProject(selected[j].Project, sum.Project) && nearSets(fingerprints[j], fingerprints[i]) {
					id := selected[j].ID
					dupOf = &id
					break
				}
			}
			age := nowMs - sum.UpdatedAtMs
			if age < 0 {
				age = 0
			}
			stale := age >= staleThreshold
			if dupOf != nil || stale {
				candidates = append(candidates, reviewCandidate{Summary: sum, DuplicateOf: dupOf, Stale: stale})
			}
		}
		return candidates, nil
	})
}

func (s storageClient) readCatalogCommand(ctx context.Context, input protocol.CommandReadMemoryCatalogPayload) (protocol.MemoryCatalog, error) {
	cfg, err := s.readSettings(ctx)
	if err != nil {
		return protocol.MemoryCatalog{}, err
	}
	var project *string
	if input.Project != nil && *input.Project != "" {
		norm, err := normalizeID(*input.Project)
		if err != nil {
			return protocol.MemoryCatalog{}, err
		}
		project = &norm
	}
	allProjects := project == nil && !input.GlobalOnly
	view := input.View
	if view == "" {
		view = "active"
	}
	switch view {
	case "active", "archived", "review", "all":
	default:
		return protocol.MemoryCatalog{}, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory catalog view")
	}

	if view == "all" {
		var anyTags []string
		if !allProjects {
			anyTags = []string{scopeTag(project)}
		}
		recs, err := s.indexed(ctx, protocol.PluginStorageSearch{
			Terms: []string{}, All: []string{}, Any: anyTags, Mode: protocol.KnowledgeModeKeyword, Weights: [2]uint16{3, 1},
		}, 0)
		if err != nil {
			return protocol.MemoryCatalog{}, err
		}
		summaries := make([]memorySummary, 0, len(recs))
		for _, rec := range recs {
			meta, err := parseStoredEntry(rec)
			if err != nil {
				return protocol.MemoryCatalog{}, err
			}
			summaries = append(summaries, meta.Summary)
		}
		slices.SortFunc(summaries, compareSummaryOrder)
		entries := make([]protocol.MemorySummary, 0, len(summaries))
		for _, item := range summaries {
			entries = append(entries, toProtocolSummary(item))
		}
		return protocol.MemoryCatalog{Settings: toProtocolSettings(cfg), Entries: entries}, nil
	}

	results, err := s.browseMemories(ctx, project, allProjects, view == "archived", input.Query)
	if err != nil {
		return protocol.MemoryCatalog{}, err
	}
	if view != "review" {
		entries := make([]protocol.MemorySummary, 0, len(results))
		for _, item := range results {
			entries = append(entries, toProtocolSummary(item))
		}
		return protocol.MemoryCatalog{Settings: toProtocolSettings(cfg), Entries: entries}, nil
	}

	reviews, err := s.reviewCatalog(ctx, cfg, project, allProjects)
	if err != nil {
		return protocol.MemoryCatalog{}, mapMemoryError(err)
	}
	byID := make(map[string]reviewCandidate, len(reviews))
	for _, cand := range reviews {
		byID[cand.Summary.ID] = cand
	}
	entries := make([]protocol.MemorySummary, 0, len(reviews))
	for _, item := range results {
		cand, matched := byID[item.ID]
		if !matched {
			continue
		}
		summary := toProtocolSummary(item)
		summary.DuplicateOf = cand.DuplicateOf
		summary.NeedsReview = true
		entries = append(entries, summary)
	}
	return protocol.MemoryCatalog{Settings: toProtocolSettings(cfg), Entries: entries}, nil
}

func validateControllerMemoryText(title, body string) error {
	if title == "" || len(title) > 160 {
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory title")
	}
	for _, r := range title {
		if unicode.Is(unicode.Cc, r) {
			return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory title")
		}
	}
	if strings.TrimSpace(body) == "" || len(body) > 8192 || strings.ContainsRune(body, 0) {
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory body")
	}
	return nil
}

func extractTransactionStorageEntries(outputs []protocol.Output) (entries []protocol.PluginStorageEntry, catalogRev uint64) {
	for _, out := range outputs {
		if out.Kind != "plugin_value" {
			continue
		}
		var rec protocol.PluginStorageEntry
		switch v := out.Data.(type) {
		case protocol.PluginStorageEntry:
			rec = v
		case *protocol.PluginStorageEntry:
			if v != nil {
				rec = *v
			}
		default:
			raw, _ := json.Marshal(v)
			_ = json.Unmarshal(raw, &rec)
		}
		if rec.Key == headKey && rec.Revision > catalogRev {
			catalogRev = rec.Revision
		}
		entries = append(entries, rec)
	}
	return entries, catalogRev
}

func (s storageClient) saveEntryCommand(ctx context.Context, input protocol.CommandSaveMemoryEntryPayload) (protocol.MemoryEntry, uint64, error) {
	title := strings.TrimSpace(input.Title)
	if err := validateControllerMemoryText(title, input.Body); err != nil {
		return protocol.MemoryEntry{}, 0, err
	}
	var project *string
	if input.Project != nil && *input.Project != "" {
		norm, err := normalizeID(*input.Project)
		if err != nil {
			return protocol.MemoryEntry{}, 0, err
		}
		project = &norm
	}
	id := ""
	if input.ID != nil && *input.ID != "" {
		norm, err := normalizeID(*input.ID)
		if err != nil {
			return protocol.MemoryEntry{}, 0, err
		}
		id = norm
	} else {
		if input.ExpectedRevision != 0 {
			return protocol.MemoryEntry{}, 0, failFault(protocol.ErrorCodeInvalidRequest, "new memory cannot specify expected_revision")
		}
		id = newUUIDv4()
	}

	var mergeSources []mergeSourceRef
	if len(input.MergeSources) > 0 {
		mergeSources = make([]mergeSourceRef, 0, len(input.MergeSources))
		for _, src := range input.MergeSources {
			norm, err := normalizeID(src.ID)
			if err != nil {
				return protocol.MemoryEntry{}, 0, err
			}
			mergeSources = append(mergeSources, mergeSourceRef{ID: norm, Revision: int64(src.Revision)})
		}
	}

	entry := memoryEntry{
		Summary: memorySummary{
			ID:       id,
			Project:  project,
			Title:    title,
			Kind:     input.Kind,
			Archived: input.Archived,
			Revision: int64(input.ExpectedRevision),
		},
		Body: input.Body,
	}

	for attempt := 0; attempt < 8; attempt++ {
		var ops []protocol.PluginTransactionOperation
		var err error
		if len(mergeSources) > 0 {
			ops, err = s.mergeOperations(ctx, entry, mergeSources, nil)
		} else {
			ops, err = s.putOperations(ctx, entry, int64(input.ExpectedRevision), nil)
		}
		if err != nil {
			return protocol.MemoryEntry{}, 0, err
		}
		outputs, txErr := s.transaction(ctx, ops)
		if txErr != nil {
			if isValueConflict(txErr) && attempt < 7 {
				continue
			}
			return protocol.MemoryEntry{}, 0, txErr
		}
		recs, catalogRev := extractTransactionStorageEntries(outputs)
		targetKey := entryKey(id)
		for _, rec := range recs {
			if rec.Key == targetKey && rec.Present {
				meta, err := parseStoredEntry(rec)
				if err != nil {
					return protocol.MemoryEntry{}, 0, err
				}
				return protocol.MemoryEntry{
					Summary: toProtocolSummary(meta.Summary),
					Body:    input.Body,
				}, catalogRev, nil
			}
		}
		return protocol.MemoryEntry{}, 0, failFault(protocol.ErrorCodeOutcomeUnknown, "memory outcome is unavailable")
	}
	return protocol.MemoryEntry{}, 0, failFault(protocol.ErrorCodeRevisionConflict, "memory catalog changed")
}

func (s storageClient) deleteEntryCommand(ctx context.Context, input protocol.CommandDeleteMemoryEntryPayload) (string, uint64, error) {
	id, err := normalizeID(input.ID)
	if err != nil {
		return "", 0, err
	}
	for attempt := 0; attempt < 8; attempt++ {
		ops, err := s.removeOperations(ctx, id, int64(input.ExpectedRevision), nil)
		if err != nil {
			return "", 0, err
		}
		outputs, txErr := s.transaction(ctx, ops)
		if txErr != nil {
			if isValueConflict(txErr) && attempt < 7 {
				continue
			}
			return "", 0, txErr
		}
		_, catalogRev := extractTransactionStorageEntries(outputs)
		return id, catalogRev, nil
	}
	return "", 0, failFault(protocol.ErrorCodeRevisionConflict, "memory catalog changed")
}

func (s storageClient) saveSettingsCommand(ctx context.Context, input protocol.CommandSaveMemorySettingsPayload) (protocol.MemorySettings, uint64, error) {
	if input.ContextBytes < 2048 || input.ContextBytes > 65536 || input.ReviewAfterDays < 1 || input.ReviewAfterDays > 3650 {
		return protocol.MemorySettings{}, 0, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory settings")
	}
	payload := map[string]any{
		"v": 1,
		"settings": map[string]any{
			"enabled":           input.Enabled,
			"auto_write":        input.AutoWrite,
			"context_bytes":     input.ContextBytes,
			"review_after_days": input.ReviewAfterDays,
		},
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		return protocol.MemorySettings{}, 0, err
	}
	for attempt := 0; attempt < 8; attempt++ {
		cur, err := s.readSettings(ctx)
		if err != nil {
			return protocol.MemorySettings{}, 0, err
		}
		if cur.Revision != input.ExpectedRevision {
			return protocol.MemorySettings{}, 0, failFault(protocol.ErrorCodeRevisionConflict, "memory settings changed")
		}
		h, err := s.head(ctx)
		if err != nil {
			return protocol.MemorySettings{}, 0, err
		}
		ops := []protocol.PluginTransactionOperation{
			makeWriteOp(h.record, h.value),
			{
				Kind: "write",
				Data: protocol.PluginTransactionOperationWritePayload{
					Key:              settingsKey,
					Value:            raw,
					ExpectedRevision: input.ExpectedRevision,
					Scope:            protocol.PluginStorageScopeNode,
				},
			},
		}
		outputs, txErr := s.transaction(ctx, ops)
		if txErr != nil {
			if isValueConflict(txErr) && attempt < 7 {
				continue
			}
			return protocol.MemorySettings{}, 0, txErr
		}
		recs, catalogRev := extractTransactionStorageEntries(outputs)
		for _, rec := range recs {
			if rec.Key == settingsKey && rec.Present {
				saved := protocol.MemorySettings{
					Revision:        rec.Revision,
					Enabled:         input.Enabled,
					AutoWrite:       input.AutoWrite,
					ContextBytes:    input.ContextBytes,
					ReviewAfterDays: input.ReviewAfterDays,
				}
				return saved, catalogRev, nil
			}
		}
		return protocol.MemorySettings{}, 0, failFault(protocol.ErrorCodeOutcomeUnknown, "memory settings outcome is unavailable")
	}
	return protocol.MemorySettings{}, 0, failFault(protocol.ErrorCodeRevisionConflict, "memory settings changed")
}
