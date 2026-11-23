package memorynode

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const storageNamespace = "memory"

type snapshotChangedError struct{ msg string }

func (e snapshotChangedError) Error() string { return e.msg }

func isSnapshotChanged(err error) bool {
	var target snapshotChangedError
	return errors.As(err, &target)
}

type recordChangedError struct{ ids []string }

func (e recordChangedError) Error() string { return "memory revision changed" }

type equivalentMemoryError struct{ id string }

func (e equivalentMemoryError) Error() string { return "equivalent memory already exists" }

type storageClient struct {
	host moduleapi.Host
	inv  *store.AgentInvocation
}

func (s storageClient) get(ctx context.Context, key string) (protocol.PluginStorageEntry, error) {
	return s.host.PluginStorageGet(ctx, s.inv, storageNamespace, key)
}

func (s storageClient) search(ctx context.Context, q protocol.PluginStorageSearch) (protocol.PluginStorageSearchPage, error) {
	q.Scope = protocol.PluginStorageScopeNode
	return s.host.PluginStorageSearch(ctx, s.inv, storageNamespace, q)
}

func (s storageClient) transaction(ctx context.Context, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	return s.host.PluginStorageTransaction(ctx, s.inv, storageNamespace, ops)
}

func (s storageClient) readSettings(ctx context.Context) (memorySettings, error) {
	rec, err := s.get(ctx, settingsKey)
	if err != nil {
		return memorySettings{}, err
	}
	if !rec.Present {
		return defaultMemorySettings(), nil
	}
	var doc struct {
		V        int `json:"v"`
		Settings struct {
			Enabled         *bool `json:"enabled"`
			AutoWrite       *bool `json:"auto_write"`
			ContextBytes    *int  `json:"context_bytes"`
			ReviewAfterDays *int  `json:"review_after_days"`
		} `json:"settings"`
	}
	if json.Unmarshal(rec.Value, &doc) != nil || doc.V != 1 ||
		doc.Settings.Enabled == nil || doc.Settings.AutoWrite == nil ||
		doc.Settings.ContextBytes == nil || doc.Settings.ReviewAfterDays == nil {
		return memorySettings{}, failFault(protocol.ErrorCodeInvalidRequest, "memory settings are incompatible")
	}
	cfg := memorySettings{
		Revision:        rec.Revision,
		Enabled:         *doc.Settings.Enabled,
		AutoWrite:       *doc.Settings.AutoWrite,
		ContextBytes:    *doc.Settings.ContextBytes,
		ReviewAfterDays: *doc.Settings.ReviewAfterDays,
	}
	if cfg.ContextBytes < 2048 || cfg.ContextBytes > 65536 || cfg.ReviewAfterDays < 1 || cfg.ReviewAfterDays > 3650 {
		return memorySettings{}, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory settings")
	}
	return cfg, nil
}

type headRecord struct {
	record protocol.PluginStorageEntry
	value  memoryCatalog
}

func (s storageClient) head(ctx context.Context) (headRecord, error) {
	rec, err := s.get(ctx, headKey)
	if err != nil {
		return headRecord{}, err
	}
	var parsed *memoryCatalog
	if rec.Present {
		var cat memoryCatalog
		if err := json.Unmarshal(rec.Value, &cat); err != nil {
			return headRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
		}
		parsed = &cat
	}
	cat, err := validateCatalog(parsed)
	if err != nil {
		return headRecord{}, err
	}
	return headRecord{record: rec, value: cat}, nil
}

func (s storageClient) unchanged(ctx context.Context, rev uint64) error {
	h, err := s.head(ctx)
	if err != nil {
		return err
	}
	if h.record.Revision != rev {
		return snapshotChangedError{msg: "memory catalog changed"}
	}
	return nil
}

func stableRead[T any](fn func() (T, error)) (T, error) {
	var zero T
	for attempt := 0; ; attempt++ {
		val, err := fn()
		if err == nil {
			return val, nil
		}
		if attempt == 2 || !isSnapshotChanged(err) {
			return zero, err
		}
	}
}

func parseStoredEntry(rec protocol.PluginStorageEntry) (storedEntry, error) {
	if !rec.Present {
		return storedEntry{}, failFault(protocol.ErrorCodeNotFound, "memory does not exist")
	}
	var val storedEntry
	if err := json.Unmarshal(rec.Value, &val); err != nil {
		return storedEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	if val.V != 1 || !pageIDRE.MatchString(val.BodyPage) || !hash16RE.MatchString(val.Hash) || !strings.HasPrefix(rec.Key, entryPrefix) {
		return storedEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	if err := validateSummary(val.Summary); err != nil {
		return storedEntry{}, err
	}
	if val.BodyRevision < 1 || val.BodyRevision > val.Summary.Revision {
		return storedEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	if entryKey(val.Summary.ID) != rec.Key {
		return storedEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	return val, nil
}

func (s storageClient) record(ctx context.Context, id string, project *string) (protocol.PluginStorageEntry, error) {
	norm, err := normalizeID(id)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	rec, err := s.get(ctx, entryKey(norm))
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if rec.Present {
		meta, err := parseStoredEntry(rec)
		if err != nil {
			return protocol.PluginStorageEntry{}, err
		}
		if !visibleInProject(meta.Summary, project) {
			return protocol.PluginStorageEntry{}, failFault(protocol.ErrorCodePermissionDenied, "memory belongs to another project")
		}
	}
	return rec, nil
}

type bodyPageRecord struct {
	id     string
	record protocol.PluginStorageEntry
	value  storedBodyPage
}

func (s storageClient) page(ctx context.Context, id string) (bodyPageRecord, error) {
	rec, err := s.get(ctx, bodyKey(id))
	if err != nil {
		return bodyPageRecord{}, err
	}
	val := storedBodyPage{V: 1, Bodies: map[string]storedBody{}}
	if rec.Present {
		if err := json.Unmarshal(rec.Value, &val); err != nil || val.V != 1 || val.Bodies == nil || len(val.Bodies) > pageLimit {
			return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
		}
		for bid, content := range val.Bodies {
			if _, err := normalizeID(bid); err != nil || content.Revision < 1 {
				return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
			}
		}
	}
	return bodyPageRecord{id: id, record: rec, value: val}, nil
}

func resolveBody(meta storedEntry, source bodyPageRecord) (memoryEntry, error) {
	content, ok := source.value.Bodies[meta.Summary.ID]
	if !ok {
		return memoryEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	if content.Revision != meta.BodyRevision {
		return memoryEntry{}, failFault(protocol.ErrorCodeRevisionConflict, "memory body changed")
	}
	entry := memoryEntry{Summary: meta.Summary, Body: content.Body}
	if err := validateEntry(entry); err != nil {
		return memoryEntry{}, err
	}
	return entry, nil
}

func (s storageClient) readMemory(ctx context.Context, id string, project *string) (memoryEntry, error) {
	return stableRead(func() (memoryEntry, error) {
		h, err := s.head(ctx)
		if err != nil {
			return memoryEntry{}, err
		}
		rec, err := s.record(ctx, id, project)
		if err != nil {
			return memoryEntry{}, err
		}
		if !rec.Present {
			if err := s.unchanged(ctx, h.record.Revision); err != nil {
				return memoryEntry{}, err
			}
			return memoryEntry{}, failFault(protocol.ErrorCodeNotFound, "memory does not exist")
		}
		meta, err := parseStoredEntry(rec)
		if err != nil {
			return memoryEntry{}, err
		}
		src, err := s.page(ctx, meta.BodyPage)
		if err != nil {
			return memoryEntry{}, err
		}
		if err := s.unchanged(ctx, h.record.Revision); err != nil {
			return memoryEntry{}, err
		}
		return resolveBody(meta, src)
	})
}

func (s storageClient) clock(ctx context.Context) (int64, error) {
	res, err := s.search(ctx, protocol.PluginStorageSearch{
		Terms: []string{}, All: []string{"state:clock"}, Any: []string{}, Mode: protocol.KnowledgeModeKeyword,
		Weights: [2]uint16{3, 1}, Offset: 0, Limit: 1,
	})
	if err != nil {
		return 0, err
	}
	return int64(res.NowMs), nil
}

func (s storageClient) indexed(ctx context.Context, filter protocol.PluginStorageSearch, limit int) ([]protocol.PluginStorageEntry, error) {
	var values []protocol.PluginStorageEntry
	offset := uint16(0)
	for {
		batchLimit := uint16(100)
		if limit > 0 {
			rem := limit - len(values)
			if rem <= 0 {
				break
			}
			if rem < int(batchLimit) {
				batchLimit = uint16(rem)
			}
		}
		q := filter
		q.Offset = offset
		q.Limit = batchLimit
		res, err := s.search(ctx, q)
		if err != nil {
			return nil, err
		}
		for _, entry := range res.Entries {
			if _, err := parseStoredEntry(entry); err != nil {
				return nil, err
			}
			values = append(values, entry)
		}
		if (limit > 0 && len(values) >= limit) || res.Next == nil {
			break
		}
		if *res.Next <= offset {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory search cursor is invalid")
		}
		offset = *res.Next
	}
	return values, nil
}

func (s storageClient) findEntries(ctx context.Context, project *string, query string, keywordOnly bool) ([]protocol.PluginStorageEntry, error) {
	trimmed := strings.TrimSpace(query)
	all := []string{"state:active"}
	anyTags := []string{scopeTag(nil)}
	if project != nil {
		anyTags = append(anyTags, scopeTag(project))
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
		if !meta.Summary.Archived && visibleInProject(meta.Summary, project) {
			return []protocol.PluginStorageEntry{rec}, nil
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
	if trimmed != "" && !keywordOnly {
		q.Query = &query
		q.Mode = protocol.KnowledgeModeHybrid
	}
	return s.indexed(ctx, q, 20)
}

func (s storageClient) searchMemories(ctx context.Context, project *string, query string, keywordOnly bool) ([]memoryEntry, error) {
	return stableRead(func() ([]memoryEntry, error) {
		h, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		recs, err := s.findEntries(ctx, project, query, keywordOnly)
		if err != nil {
			return nil, err
		}
		metas := make([]storedEntry, 0, len(recs))
		pages := map[string]bodyPageRecord{}
		for _, rec := range recs {
			meta, err := parseStoredEntry(rec)
			if err != nil {
				return nil, err
			}
			metas = append(metas, meta)
			if _, loaded := pages[meta.BodyPage]; !loaded {
				p, err := s.page(ctx, meta.BodyPage)
				if err != nil {
					return nil, err
				}
				pages[meta.BodyPage] = p
			}
		}
		if err := s.unchanged(ctx, h.record.Revision); err != nil {
			return nil, err
		}
		out := make([]memoryEntry, 0, len(metas))
		for _, meta := range metas {
			entry, err := resolveBody(meta, pages[meta.BodyPage])
			if err != nil {
				return nil, err
			}
			out = append(out, entry)
		}
		return out, nil
	})
}

type reviewCandidate struct {
	Summary     memorySummary `json:"summary"`
	DuplicateOf *string       `json:"duplicate_of"`
	Stale       bool          `json:"stale"`
}

func (s storageClient) reviewMemories(ctx context.Context, cfg memorySettings, project *string) ([]reviewCandidate, error) {
	return stableRead(func() ([]reviewCandidate, error) {
		before, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		anyTags := []string{scopeTag(nil)}
		if project != nil {
			anyTags = append(anyTags, scopeTag(project))
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
			if !meta.Summary.Archived && visibleInProject(meta.Summary, project) {
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

func makeWriteOp(rec protocol.PluginStorageEntry, value any) protocol.PluginTransactionOperation {
	raw, _ := json.Marshal(value)
	return protocol.PluginTransactionOperation{
		Kind: "write",
		Data: protocol.PluginTransactionOperationWritePayload{
			Key: rec.Key, Value: raw, ExpectedRevision: rec.Revision, Scope: protocol.PluginStorageScopeNode,
		},
	}
}

func makeRemoveOp(rec protocol.PluginStorageEntry) protocol.PluginTransactionOperation {
	return protocol.PluginTransactionOperation{
		Kind: "remove",
		Data: protocol.PluginTransactionOperationRemovePayload{
			Key: rec.Key, ExpectedRevision: rec.Revision, Scope: protocol.PluginStorageScopeNode,
		},
	}
}

func makeIndexOp(rec protocol.PluginStorageEntry, entry memoryEntry, pageID string, bodyRev int64) protocol.PluginTransactionOperation {
	hash := equivalenceHash(entry.Body)
	val := storedEntry{
		V:            1,
		Summary:      entry.Summary,
		Hash:         hash,
		BodyPage:     pageID,
		BodyRevision: bodyRev,
	}
	raw, _ := json.Marshal(val)
	stateTag := "state:active"
	if entry.Summary.Archived {
		stateTag = "state:archived"
	}
	escapedID := strings.ReplaceAll(strings.ReplaceAll(entry.Summary.ID, "~", "~0"), "/", "~1")
	return protocol.PluginTransactionOperation{
		Kind: "index",
		Data: protocol.PluginTransactionOperationIndexPayload{
			Key:   rec.Key,
			Value: raw,
			Index: protocol.PluginStorageIndex{
				Fields: [2]string{entry.Summary.Title, strings.Join(terms(entry.Body), " ")},
				Tags:   []string{stateTag, scopeTag(entry.Summary.Project), "equivalent:" + hash},
				Order:  entry.Summary.UpdatedAtMs,
				Source: &protocol.PluginStorageSource{
					Key:     bodyKey(pageID),
					Pointer: "/bodies/" + escapedID + "/body",
				},
			},
			ExpectedRevision: rec.Revision,
			Scope:            protocol.PluginStorageScopeNode,
		},
	}
}

func checkExpectedEntry(rec protocol.PluginStorageEntry, expectedRev int64) error {
	var curRev int64
	if rec.Present {
		meta, err := parseStoredEntry(rec)
		if err != nil {
			return err
		}
		curRev = meta.Summary.Revision
	}
	if curRev != expectedRev || (!rec.Present && rec.Revision != 0) {
		return recordChangedError{ids: []string{strings.TrimPrefix(rec.Key, entryPrefix)}}
	}
	return nil
}

func (s storageClient) checkEquivalent(ctx context.Context, entry memoryEntry, excluded map[string]bool, headRev uint64) error {
	if entry.Summary.Archived {
		return nil
	}
	hash := equivalenceHash(entry.Body)
	cands, err := s.indexed(ctx, protocol.PluginStorageSearch{
		Terms:   []string{},
		All:     []string{"state:active", scopeTag(entry.Summary.Project), "equivalent:" + hash},
		Any:     []string{},
		Mode:    protocol.KnowledgeModeKeyword,
		Weights: [2]uint16{3, 1},
	}, 0)
	if err != nil {
		return err
	}
	wantNorm := normalizedText(entry.Body)
	for _, cand := range cands {
		meta, err := parseStoredEntry(cand)
		if err != nil {
			return err
		}
		if excluded[meta.Summary.ID] {
			continue
		}
		src, err := s.page(ctx, meta.BodyPage)
		if err != nil {
			return err
		}
		if err := s.unchanged(ctx, headRev); err != nil {
			return err
		}
		full, err := resolveBody(meta, src)
		if err != nil {
			return err
		}
		if normalizedText(full.Body) == wantNorm {
			return equivalentMemoryError{id: meta.Summary.ID}
		}
	}
	return nil
}

func (s storageClient) ownedPage(ctx context.Context, h headRecord, pageID string) (bodyPageRecord, error) {
	stored, err := s.page(ctx, pageID)
	if err != nil {
		return bodyPageRecord{}, err
	}
	if err := s.unchanged(ctx, h.record.Revision); err != nil {
		return bodyPageRecord{}, err
	}
	expectedCount, ok := h.value.Pages[pageID]
	if !ok || len(stored.value.Bodies) != expectedCount {
		return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	return stored, nil
}

func (s storageClient) allocatePage(ctx context.Context, h *headRecord, id string) (bodyPageRecord, error) {
	selected := ""
	// Deterministic selection: sort existing page keys for stable allocation.
	keys := make([]string, 0, len(h.value.Pages))
	for k := range h.value.Pages {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	for _, k := range keys {
		if h.value.Pages[k] < pageLimit {
			selected = k
			break
		}
	}
	if selected == "" {
		if h.value.Next >= maxSafeInt {
			return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory page identifiers exhausted")
		}
		selected = strconv.FormatInt(h.value.Next, 10)
		h.value.Next++
	}
	stored, err := s.page(ctx, selected)
	if err != nil {
		return bodyPageRecord{}, err
	}
	if err := s.unchanged(ctx, h.record.Revision); err != nil {
		return bodyPageRecord{}, err
	}
	if len(stored.value.Bodies) != h.value.Pages[selected] {
		return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	if _, exists := stored.value.Bodies[id]; exists {
		return bodyPageRecord{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	h.value.Pages[selected]++
	return stored, nil
}

func (s storageClient) putOperations(ctx context.Context, input memoryEntry, expectedRev int64, project *string) ([]protocol.PluginTransactionOperation, error) {
	return stableRead(func() ([]protocol.PluginTransactionOperation, error) {
		if err := validateEntry(input); err != nil {
			return nil, err
		}
		if input.Summary.Revision != expectedRev {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory revision")
		}
		h, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		stored, err := s.record(ctx, input.Summary.ID, project)
		if err != nil {
			return nil, err
		}
		nowMs, err := s.clock(ctx)
		if err != nil {
			return nil, err
		}
		cfg, err := s.readSettings(ctx)
		if err != nil {
			return nil, err
		}
		if !cfg.Enabled || !cfg.AutoWrite {
			return nil, failFault(protocol.ErrorCodePermissionDenied, "memory access is disabled")
		}
		if err := checkExpectedEntry(stored, expectedRev); err != nil {
			return nil, err
		}
		var previous *storedEntry
		if stored.Present {
			meta, err := parseStoredEntry(stored)
			if err != nil {
				return nil, err
			}
			previous = &meta
			if !sameProject(previous.Summary.Project, input.Summary.Project) {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory scope cannot be changed")
			}
		}
		if !visibleInProject(input.Summary, project) {
			return nil, failFault(protocol.ErrorCodePermissionDenied, "memory belongs to another project")
		}
		saved, err := nextEntry(input, nowMs)
		if err != nil {
			return nil, err
		}
		if err := s.checkEquivalent(ctx, saved, map[string]bool{saved.Summary.ID: true}, h.record.Revision); err != nil {
			return nil, err
		}
		var target bodyPageRecord
		if previous != nil {
			target, err = s.ownedPage(ctx, h, previous.BodyPage)
		} else {
			target, err = s.allocatePage(ctx, &h, saved.Summary.ID)
		}
		if err != nil {
			return nil, err
		}
		if previous != nil {
			if _, exists := target.value.Bodies[saved.Summary.ID]; !exists {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
			}
		}
		var prevSum *memorySummary
		bodyRev := saved.Summary.Revision
		if previous != nil {
			sCopy := previous.Summary
			prevSum = &sCopy
			bodyRev = previous.BodyRevision
		}
		if err := applyCapacity(&h.value, []capacityDelta{{before: prevSum, after: &saved.Summary}}); err != nil {
			return nil, err
		}
		ops := []protocol.PluginTransactionOperation{makeWriteOp(h.record, h.value)}
		existing, hasExisting := target.value.Bodies[saved.Summary.ID]
		if !hasExisting || existing.Body != saved.Body {
			bodyRev = saved.Summary.Revision
			target.value.Bodies[saved.Summary.ID] = storedBody{Revision: bodyRev, Body: saved.Body}
			ops = append(ops, makeWriteOp(target.record, target.value))
		}
		ops = append(ops, makeIndexOp(stored, saved, target.id, bodyRev))
		return ops, nil
	})
}

func (s storageClient) removeOperations(ctx context.Context, id string, expectedRev int64, project *string) ([]protocol.PluginTransactionOperation, error) {
	return stableRead(func() ([]protocol.PluginTransactionOperation, error) {
		h, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		stored, err := s.record(ctx, id, project)
		if err != nil {
			return nil, err
		}
		cfg, err := s.readSettings(ctx)
		if err != nil {
			return nil, err
		}
		if !cfg.Enabled || !cfg.AutoWrite {
			return nil, failFault(protocol.ErrorCodePermissionDenied, "memory access is disabled")
		}
		previous, err := parseStoredEntry(stored)
		if err != nil {
			return nil, err
		}
		if err := checkExpectedEntry(stored, expectedRev); err != nil {
			return nil, err
		}
		target, err := s.ownedPage(ctx, h, previous.BodyPage)
		if err != nil {
			return nil, err
		}
		if _, exists := target.value.Bodies[previous.Summary.ID]; !exists {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
		}
		delete(target.value.Bodies, previous.Summary.ID)
		h.value.Pages[previous.BodyPage]--
		if h.value.Pages[previous.BodyPage] == 0 {
			delete(h.value.Pages, previous.BodyPage)
		}
		if err := applyCapacity(&h.value, []capacityDelta{{before: &previous.Summary, after: nil}}); err != nil {
			return nil, err
		}
		var pageOp protocol.PluginTransactionOperation
		if len(target.value.Bodies) > 0 {
			pageOp = makeWriteOp(target.record, target.value)
		} else {
			pageOp = makeRemoveOp(target.record)
		}
		return []protocol.PluginTransactionOperation{
			makeWriteOp(h.record, h.value),
			pageOp,
			makeRemoveOp(stored),
		}, nil
	})
}

type mergeSourceRef struct {
	ID       string `json:"id"`
	Revision int64  `json:"revision"`
}

func (s storageClient) mergeOperations(ctx context.Context, input memoryEntry, sources []mergeSourceRef, project *string) ([]protocol.PluginTransactionOperation, error) {
	return stableRead(func() ([]protocol.PluginTransactionOperation, error) {
		if err := validateEntry(input); err != nil {
			return nil, err
		}
		if input.Summary.Archived || len(sources) == 0 {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory consolidation")
		}
		h, err := s.head(ctx)
		if err != nil {
			return nil, err
		}
		targetRec, err := s.record(ctx, input.Summary.ID, project)
		if err != nil {
			return nil, err
		}
		nowMs, err := s.clock(ctx)
		if err != nil {
			return nil, err
		}
		cfg, err := s.readSettings(ctx)
		if err != nil {
			return nil, err
		}
		if !cfg.Enabled || !cfg.AutoWrite {
			return nil, failFault(protocol.ErrorCodePermissionDenied, "memory access is disabled")
		}
		if err := checkExpectedEntry(targetRec, input.Summary.Revision); err != nil {
			return nil, err
		}
		previous, err := parseStoredEntry(targetRec)
		if err != nil {
			return nil, err
		}
		if previous.Summary.Archived {
			return nil, recordChangedError{ids: []string{previous.Summary.ID}}
		}
		if !sameProject(previous.Summary.Project, input.Summary.Project) {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory scope cannot be changed")
		}
		excluded := map[string]bool{input.Summary.ID: true}
		type archivedSource struct {
			stored protocol.PluginStorageEntry
			meta   storedEntry
			entry  memoryEntry
		}
		originals := make([]archivedSource, 0, len(sources))
		for _, src := range sources {
			id, err := normalizeID(src.ID)
			if err != nil {
				return nil, err
			}
			if excluded[id] {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "consolidation requires distinct memories in the same scope")
			}
			excluded[id] = true
			rec, err := s.record(ctx, id, project)
			if err != nil {
				return nil, err
			}
			if err := checkExpectedEntry(rec, src.Revision); err != nil {
				return nil, err
			}
			meta, err := parseStoredEntry(rec)
			if err != nil {
				return nil, err
			}
			if !sameProject(meta.Summary.Project, input.Summary.Project) {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "consolidation requires distinct memories in the same scope")
			}
			if meta.Summary.Archived {
				return nil, recordChangedError{ids: []string{meta.Summary.ID}}
			}
			p, err := s.ownedPage(ctx, h, meta.BodyPage)
			if err != nil {
				return nil, err
			}
			orig, err := resolveBody(meta, p)
			if err != nil {
				return nil, err
			}
			orig.Summary.Archived = true
			archived, err := nextEntry(orig, nowMs)
			if err != nil {
				return nil, err
			}
			originals = append(originals, archivedSource{stored: rec, meta: meta, entry: archived})
		}
		saved, err := nextEntry(input, nowMs)
		if err != nil {
			return nil, err
		}
		if err := s.checkEquivalent(ctx, saved, excluded, h.record.Revision); err != nil {
			return nil, err
		}
		targetPage, err := s.ownedPage(ctx, h, previous.BodyPage)
		if err != nil {
			return nil, err
		}
		if _, exists := targetPage.value.Bodies[saved.Summary.ID]; !exists {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
		}
		targetPage.value.Bodies[saved.Summary.ID] = storedBody{Revision: saved.Summary.Revision, Body: saved.Body}
		deltas := make([]capacityDelta, 0, len(originals)+1)
		for i := range originals {
			b := originals[i].meta.Summary
			a := originals[i].entry.Summary
			deltas = append(deltas, capacityDelta{before: &b, after: &a})
		}
		deltas = append(deltas, capacityDelta{before: &previous.Summary, after: &saved.Summary})
		if err := applyCapacity(&h.value, deltas); err != nil {
			return nil, err
		}
		ops := make([]protocol.PluginTransactionOperation, 0, len(originals)+3)
		ops = append(ops, makeWriteOp(h.record, h.value), makeWriteOp(targetPage.record, targetPage.value))
		for _, orig := range originals {
			ops = append(ops, makeIndexOp(orig.stored, orig.entry, orig.meta.BodyPage, orig.meta.BodyRevision))
		}
		ops = append(ops, makeIndexOp(targetRec, saved, previous.BodyPage, saved.Summary.Revision))
		return ops, nil
	})
}
