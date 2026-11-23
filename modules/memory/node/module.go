// Package memorynode provides the compile-time native Node memory agent tools
// (search_memory, review_memories, save_memory, forget_memory,
// consolidate_memories) along with native per-turn recall and read-authority
// tracking. It shares the exact "memory" personal storage namespace and
// semantic index format with the Memory settings UI package.
package memorynode

import (
	"context"
	"crypto/rand"
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed tools.json
var catalogJSON []byte

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

type module struct{}

// Module returns the native Node module providing memory recall and management
// tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "memory" }
func (module) ReplacesPlugins() []string { return []string{"memory"} }

func (module) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

type turnState struct {
	mu        sync.Mutex
	settings  memorySettings
	project   *string
	mode      protocol.WorkMode
	remaining int
	reads     []readAuthority
	lastCall  string
	storage   storageClient
}

func (module) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	// If a legacy memory plugin is installed and explicitly disabled by the
	// user, honor that toggle. Once the plugin is retired, memory runs natively
	// using the built-in "memory" storage namespace and memory/settings toggle.
	if installed, _, err := host.PluginInstalled(ctx, storageNamespace); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, storageNamespace); !captured {
			return nil, nil
		}
	}

	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	var available []moduleapi.ToolSpec
	for _, spec := range catalog.Tools {
		if spec.Available(inv) {
			available = append(available, spec)
		}
	}
	if len(available) == 0 {
		return nil, nil
	}

	client := storageClient{host: host, inv: inv}
	settings, err := client.readSettings(ctx)
	if err != nil {
		return nil, err
	}
	if !settings.Enabled {
		return nil, nil
	}

	var projectID *string
	if inv.Worktree.Project != nil {
		s := inv.Worktree.Project.String()
		projectID = &s
	}

	// Only a person's own message triggers recall and the save cue (same policy
	// as pluginTurnInput in plugin_tools_script.go).
	message := ""
	automatic := inv.Run.Notice || inv.Run.Continuation != nil
	if !automatic && inv.Session.Delegation == nil {
		rawMsg := inv.Message.Text
		if strings.TrimSpace(rawMsg) != "" {
			end := min(len(rawMsg), 4096)
			for end > 0 && end < len(rawMsg) && !utf8.RuneStart(rawMsg[end]) {
				end--
			}
			message = rawMsg[:end]
		}
	}

	state := &turnState{
		settings:  settings,
		project:   projectID,
		mode:      inv.Turn.Config.Mode,
		remaining: settings.ContextBytes,
		storage:   client,
	}

	var parts []string
	if settings.Enabled {
		var relevant []memoryEntry
		if strings.TrimSpace(message) != "" {
			// Best-effort keyword recall; failures add no context rather than failing the turn.
			if found, err := client.searchMemories(ctx, projectID, message, true); err == nil {
				relevant = found
			}
		}
		write := settings.AutoWrite && inv.Turn.Config.Mode != protocol.WorkModePlan
		parts = buildTurnParts(relevant, message, settings, write)
		used := 0
		for _, p := range parts {
			used += len(p)
		}
		state.remaining = max(0, state.remaining-used)
	}

	beforeModel := state.beforeModel(inv.Run.Turn)
	tools := make([]engine.Tool, 0, len(available))
	for _, spec := range available {
		isReadTool := spec.Name == "search_memory" || spec.Name == "review_memories"
		if !settings.AutoWrite && !isReadTool {
			continue
		}
		spec.ReadOnly = isReadTool
		spec.Concurrent = false
		var exec func(context.Context, canonical.ToolRequest) engine.ToolResult
		switch spec.Name {
		case "search_memory":
			exec = state.execSearch
		case "review_memories":
			exec = state.execReview
		case "save_memory":
			exec = state.execSave
		case "forget_memory":
			exec = state.execForget
		case "consolidate_memories":
			exec = state.execConsolidate
		default:
			continue
		}
		tool, err := moduleapi.BuildCustomTool(spec, catalog.Instructions, exec)
		if err != nil {
			return nil, err
		}
		tool.ContextParts = parts
		tool.BeforeModel = beforeModel
		if !isReadTool {
			// Storage writes are confined to the memory namespace and approved
			// automatically by the storage scope (except in plan mode).
			tool.Approval = true
			tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, _ canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
				return host.ConfigureStorageApproval(ctx, db, inv.Run.Turn, value)
			}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func (s *turnState) beforeModel(turn protocol.TurnID) func(context.Context, []canonical.Record, engine.ToolResultReader) (bool, error) {
	return func(_ context.Context, records []canonical.Record, read engine.ToolResultReader) (bool, error) {
		s.mu.Lock()
		lastCall := s.lastCall
		s.mu.Unlock()

		var collected []readAuthority
		newest := ""
		for i := len(records) - 1; i >= 0; i-- {
			rec := records[i]
			if rec.Turn != turn {
				continue
			}
			if rec.Metadata.Input != nil && rec.Provenance.Author == "user" {
				break
			}
			msg := rec.Message()
			if msg == nil {
				continue
			}
			for j := len(msg.Content) - 1; j >= 0; j-- {
				resp, ok := msg.Content[j].Data.(*canonical.ToolResponse)
				if !ok {
					continue
				}
				if newest == "" {
					newest = resp.ID
					if newest != "" && newest == lastCall {
						return false, nil
					}
				}
				bare, match := isMemoryToolName(resp.Name)
				if !match {
					continue
				}
				body, err := read(resp)
				if err != nil {
					return false, err
				}
				collected = append(collected, extractReads(bare, body)...)
			}
		}
		s.mu.Lock()
		s.reads = collected
		s.lastCall = newest
		s.mu.Unlock()
		return false, nil
	}
}

func (s *turnState) checkEnabled(ctx context.Context, write bool) error {
	cur, err := s.storage.readSettings(ctx)
	if err != nil {
		return err
	}
	s.mu.Lock()
	initial := s.settings
	s.mu.Unlock()
	if !initial.Enabled || !cur.Enabled || (write && (!initial.AutoWrite || !cur.AutoWrite)) {
		return failFault(protocol.ErrorCodePermissionDenied, "memory access is disabled")
	}
	return nil
}

func errorToolResult(err error) engine.ToolResult {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return engine.ToolResult{
			Value:         map[string]any{"isError": true, "error": map[string]any{"code": fault.Code, "message": fault.Message}},
			ReturnedError: true,
		}
	}
	return engine.ToolResult{
		Value:         map[string]any{"isError": true, "error": map[string]any{"code": "internal", "message": err.Error()}},
		ReturnedError: true,
	}
}

func decodeArgs(raw json.RawMessage, allowed, required []string) (map[string]any, error) {
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
	}
	for k := range obj {
		if !slices.Contains(allowed, k) {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
		}
	}
	for _, k := range required {
		if _, exists := obj[k]; !exists {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
		}
	}
	for _, k := range []string{"title", "body", "query"} {
		if v, exists := obj[k]; exists {
			if _, ok := v.(string); !ok {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
			}
		}
	}
	for _, k := range []string{"global", "archived"} {
		if v, exists := obj[k]; exists {
			if _, ok := v.(bool); !ok {
				return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
			}
		}
	}
	if v, exists := obj["kind"]; exists {
		ks, ok := v.(string)
		if !ok || (ks != "user" && ks != "feedback" && ks != "project" && ks != "reference") {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
		}
	}
	if v, exists := obj["expected_revision"]; exists {
		rev, ok := safeInt(v)
		if !ok || rev < 0 {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
		}
	}
	if v, exists := obj["id"]; exists && v != nil {
		id, ok := v.(string)
		if !ok || !uuidRE.MatchString(id) {
			return nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid memory arguments")
		}
	}
	return obj, nil
}

func safeInt(v any) (int64, bool) {
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil && i >= -maxSafeInt && i <= maxSafeInt {
			return i, true
		}
		if f, err := n.Float64(); err == nil && !math.IsNaN(f) && !math.IsInf(f, 0) && math.Trunc(f) == f && f >= -maxSafeInt && f <= maxSafeInt {
			return int64(f), true
		}
	case float64:
		if !math.IsNaN(n) && !math.IsInf(n, 0) && math.Trunc(n) == n && n >= -maxSafeInt && n <= maxSafeInt {
			return int64(n), true
		}
	}
	return 0, false
}

func (s *turnState) execSearch(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
	args, err := decodeArgs(call.Arguments, []string{"query"}, []string{"query"})
	if err != nil {
		return errorToolResult(err)
	}
	if err := s.checkEnabled(ctx, false); err != nil {
		return errorToolResult(err)
	}
	captured, err := s.storage.searchMemories(ctx, s.project, args["query"].(string), false)
	if err != nil {
		if isSnapshotChanged(err) {
			s.mu.Lock()
			rem := s.remaining
			s.mu.Unlock()
			return engine.ToolResult{Value: map[string]any{"status": "pending", "budget_remaining_bytes": rem, "budget_limited": true}}
		}
		return errorToolResult(err)
	}
	s.mu.Lock()
	entries := retrieveEntries(captured, &s.remaining)
	rem := s.remaining
	s.mu.Unlock()
	return engine.ToolResult{Value: map[string]any{"entries": entries, "budget_remaining_bytes": rem, "budget_limited": true}}
}

func (s *turnState) execReview(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
	if _, err := decodeArgs(call.Arguments, nil, nil); err != nil {
		return errorToolResult(err)
	}
	if err := s.checkEnabled(ctx, false); err != nil {
		return errorToolResult(err)
	}
	cfg, err := s.storage.readSettings(ctx)
	if err != nil {
		return errorToolResult(err)
	}
	captured, err := s.storage.reviewMemories(ctx, cfg, s.project)
	if err != nil {
		if isSnapshotChanged(err) {
			return engine.ToolResult{Value: map[string]any{"status": "pending"}}
		}
		return errorToolResult(err)
	}
	s.mu.Lock()
	var hints []reviewCandidate
	for _, cand := range captured {
		raw, _ := json.Marshal(cand)
		size := contentCost(string(raw))
		if size > s.remaining {
			continue
		}
		s.remaining -= size
		hints = append(hints, cand)
	}
	s.mu.Unlock()
	if hints == nil {
		hints = []reviewCandidate{}
	}
	return engine.ToolResult{Value: map[string]any{"candidates": hints}}
}

type receiptSnapshot struct {
	ID              string
	StorageRevision uint64
	SummaryRevision int64
	Action          string
	Title           string
	Project         *string
	Kind            string
	Body            string
	BodyTruncated   bool
}

func captureReceipt(ops []protocol.PluginTransactionOperation, entry memoryEntry, action string, contextBytes int) *receiptSnapshot {
	id := strings.ToLower(entry.Summary.ID)
	targetKey := entryKey(id)
	var expRev uint64
	found := false
	for _, op := range ops {
		switch d := op.Data.(type) {
		case protocol.PluginTransactionOperationIndexPayload:
			if d.Key == targetKey {
				expRev, found = d.ExpectedRevision, true
			}
		case protocol.PluginTransactionOperationRemovePayload:
			if d.Key == targetKey {
				expRev, found = d.ExpectedRevision, true
			}
		}
		if found {
			break
		}
	}
	if !found {
		return nil
	}
	rem := contextBytes
	visible := retrieveEntries([]memoryEntry{entry}, &rem)
	bodyText := ""
	bodyTruncated := true
	if len(visible) > 0 && len(visible[0].Parts) > 0 {
		var dec retrievedPayload
		if json.Unmarshal([]byte(visible[0].Parts[0].Text), &dec) == nil {
			bodyText = dec.Body
			bodyTruncated = dec.BodyTruncated
		}
	}
	return &receiptSnapshot{
		ID:              id,
		StorageRevision: expRev + 1,
		SummaryRevision: entry.Summary.Revision + 1,
		Action:          action,
		Title:           entry.Summary.Title,
		Project:         entry.Summary.Project,
		Kind:            entry.Summary.Kind,
		Body:            bodyText,
		BodyTruncated:   bodyTruncated,
	}
}

func formatMutationResult(outputs []protocol.Output, snap *receiptSnapshot) (map[string]any, error) {
	var changedEntries []protocol.PluginStorageEntry
	for _, out := range outputs {
		if out.Kind != "plugin_value" {
			continue
		}
		var entry protocol.PluginStorageEntry
		switch v := out.Data.(type) {
		case protocol.PluginStorageEntry:
			entry = v
		case *protocol.PluginStorageEntry:
			if v != nil {
				entry = *v
			}
		default:
			raw, _ := json.Marshal(v)
			_ = json.Unmarshal(raw, &entry)
		}
		if strings.HasPrefix(entry.Key, entryPrefix) {
			changedEntries = append(changedEntries, entry)
		}
	}
	if len(changedEntries) == 0 {
		return nil, failFault(protocol.ErrorCodeOutcomeUnknown, "memory outcome is unavailable")
	}
	allRemoved := true
	for _, e := range changedEntries {
		if e.Present {
			allRemoved = false
			break
		}
	}
	result := map[string]any{}
	if allRemoved {
		ids := make([]string, 0, len(changedEntries))
		for _, e := range changedEntries {
			ids = append(ids, strings.TrimPrefix(e.Key, entryPrefix))
		}
		result["removed"] = ids
	} else {
		summaries := make([]memorySummary, 0, len(changedEntries))
		for _, e := range changedEntries {
			if !e.Present {
				continue
			}
			meta, err := parseStoredEntry(e)
			if err != nil {
				return nil, err
			}
			summaries = append(summaries, meta.Summary)
		}
		result["updated"] = summaries
	}
	if snap != nil {
		targetKey := entryKey(snap.ID)
		for _, e := range changedEntries {
			if e.Key != targetKey || e.Revision != snap.StorageRevision {
				continue
			}
			if e.Present != (snap.Action != "deleted") {
				break
			}
			if e.Present {
				meta, err := parseStoredEntry(e)
				if err != nil || meta.Summary.ID != snap.ID || meta.Summary.Revision != snap.SummaryRevision ||
					meta.Summary.Title != snap.Title || !sameProject(meta.Summary.Project, snap.Project) ||
					meta.Summary.Kind != snap.Kind {
					break
				}
			}
			result["receipt"] = map[string]any{
				"action": snap.Action, "title": snap.Title, "project": snap.Project,
				"kind": snap.Kind, "body": snap.Body, "body_truncated": snap.BodyTruncated,
			}
			break
		}
	}
	return result, nil
}

func (s *turnState) latestContext(ctx context.Context, ids []string) (map[string]any, error) {
	type pair struct {
		entries []memoryEntry
		deleted []string
	}
	captured, err := stableRead(func() (pair, error) {
		h, err := s.storage.head(ctx)
		if err != nil {
			return pair{}, err
		}
		seen := map[string]bool{}
		var res pair
		for _, id := range ids {
			if seen[id] {
				continue
			}
			seen[id] = true
			entry, err := s.storage.readMemory(ctx, id, s.project)
			if err != nil {
				var fault *protocol.Fault
				if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
					res.deleted = append(res.deleted, id)
					continue
				}
				return pair{}, err
			}
			res.entries = append(res.entries, entry)
		}
		if err := s.storage.unchanged(ctx, h.record.Revision); err != nil {
			return pair{}, err
		}
		return res, nil
	})
	if err != nil {
		return nil, err
	}
	s.mu.Lock()
	latest := retrieveEntries(captured.entries, &s.remaining)
	s.mu.Unlock()
	status := "deleted"
	if len(captured.entries) > 0 {
		status = "changed"
	}
	deleted := captured.deleted
	if deleted == nil {
		deleted = []string{}
	}
	return map[string]any{"status": status, "saved": false, "latest": latest, "deleted": deleted}, nil
}

func isValueConflict(err error) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeRevisionConflict &&
		fault.Details != nil && fault.Details.Kind == "plugin_value_conflict"
}

func (s *turnState) runWriter(ctx context.Context, prepare func(attempt int) (ops []protocol.PluginTransactionOperation, snap *receiptSnapshot, done map[string]any, err error)) engine.ToolResult {
	finish := func(val map[string]any, retried bool) engine.ToolResult {
		if retried {
			return engine.ToolResult{Value: map[string]any{"context": val}}
		}
		return engine.ToolResult{Value: val}
	}
	for step := 0; step <= 8; step++ {
		retried := step > 0
		if err := s.checkEnabled(ctx, true); err != nil {
			return errorToolResult(err)
		}
		if step == 8 {
			return finish(map[string]any{"status": "pending", "saved": false}, retried)
		}
		ops, snap, done, err := prepare(step)
		if err != nil {
			var rc recordChangedError
			if errors.As(err, &rc) {
				latest, lErr := s.latestContext(ctx, rc.ids)
				if lErr != nil {
					if isSnapshotChanged(lErr) {
						return finish(map[string]any{"status": "pending", "saved": false}, retried)
					}
					return errorToolResult(lErr)
				}
				return finish(latest, retried)
			}
			var eq equivalentMemoryError
			if errors.As(err, &eq) {
				latest, lErr := s.latestContext(ctx, []string{eq.id})
				if lErr != nil {
					if isSnapshotChanged(lErr) {
						return finish(map[string]any{"status": "pending", "saved": false}, retried)
					}
					return errorToolResult(lErr)
				}
				return finish(latest, retried)
			}
			if isSnapshotChanged(err) {
				return finish(map[string]any{"status": "pending", "saved": false}, retried)
			}
			return errorToolResult(err)
		}
		if done != nil {
			return finish(done, retried)
		}
		outputs, txErr := s.storage.transaction(ctx, ops)
		if txErr != nil {
			if isValueConflict(txErr) {
				continue
			}
			var fault *protocol.Fault
			if errors.As(txErr, &fault) && (fault.Code == protocol.ErrorCodeOutcomeUnknown || fault.Code == protocol.ErrorCodeCancelled) {
				return engine.ToolResult{Failure: fault}
			}
			return errorToolResult(txErr)
		}
		formatted, fErr := formatMutationResult(outputs, snap)
		if fErr != nil {
			return errorToolResult(fErr)
		}
		return engine.ToolResult{Value: formatted}
	}
	return finish(map[string]any{"status": "pending", "saved": false}, true)
}

func (s *turnState) findSimilar(ctx context.Context, entry memoryEntry) ([]memoryEntry, error) {
	combined := []rune(entry.Summary.Title + "\n" + entry.Body)
	if len(combined) > 1024 {
		combined = combined[:1024]
	}
	captured, err := s.storage.searchMemories(ctx, s.project, string(combined), true)
	if err != nil {
		if isSnapshotChanged(err) {
			return nil, err
		}
		return nil, nil
	}
	addedSet := map[string]bool{}
	for _, t := range terms(entry.Body) {
		addedSet[t] = true
	}
	covers := func(superset, subset map[string]bool) bool {
		for t := range subset {
			if !superset[t] {
				return false
			}
		}
		return true
	}
	var candidates []memoryEntry
	for _, cur := range captured {
		if sameProject(cur.Summary.Project, entry.Summary.Project) {
			candidates = append(candidates, cur)
		}
	}
	if len(addedSet) > 0 {
		var known []memoryEntry
		for _, cur := range candidates {
			curSet := map[string]bool{}
			for _, t := range terms(cur.Body) {
				curSet[t] = true
			}
			if covers(curSet, addedSet) {
				known = append(known, cur)
			}
		}
		if len(known) > 0 {
			return known, nil
		}
	}
	var related []memoryEntry
	for _, cur := range candidates {
		curSet := map[string]bool{}
		for _, t := range terms(cur.Body) {
			curSet[t] = true
		}
		if (len(curSet) > 0 && len(curSet) < len(addedSet) && len(curSet)*2 >= len(addedSet) && covers(addedSet, curSet)) ||
			isNearDuplicate(cur.Body, entry.Body) {
			related = append(related, cur)
		}
	}
	return related, nil
}

func newUUIDv4() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%08x-%04x-%04x-%04x-%012x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

func (s *turnState) execSave(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
	generatedID := ""
	return s.runWriter(ctx, func(attempt int) ([]protocol.PluginTransactionOperation, *receiptSnapshot, map[string]any, error) {
		args, err := decodeArgs(call.Arguments, []string{"id", "expected_revision", "global", "archived", "kind", "title", "body"}, []string{"kind", "title", "body"})
		if err != nil {
			return nil, nil, nil, err
		}
		globalFlag, _ := args["global"].(bool)
		archivedFlag, _ := args["archived"].(bool)
		kind := args["kind"].(string)
		title := args["title"].(string)
		bodyText := args["body"].(string)

		project := s.project
		if globalFlag {
			project = nil
		}
		idVal, hasID := args["id"]
		expVal, hasExp := args["expected_revision"]
		var expRev int64
		if hasExp {
			expRev, _ = safeInt(expVal)
		}
		id := ""
		if hasID && idVal != nil {
			id = strings.ToLower(idVal.(string))
			if !hasExp {
				return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "pass expected_revision from search_memory to update this memory")
			}
			s.mu.Lock()
			reads := slices.Clone(s.reads)
			s.mu.Unlock()
			if err := hasReadAuthority(reads, id, expRev); err != nil {
				return nil, nil, nil, err
			}
			cur, err := s.storage.readMemory(ctx, id, s.project)
			if err != nil {
				var fault *protocol.Fault
				if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
					return nil, nil, nil, recordChangedError{ids: []string{id}}
				}
				return nil, nil, nil, err
			}
			if cur.Summary.Revision != expRev {
				return nil, nil, nil, recordChangedError{ids: []string{id}}
			}
			project = cur.Summary.Project
		} else if expRev != 0 {
			return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "omit expected_revision to create a memory, or pass id to update one")
		}
		if project == nil && ((!globalFlag && id == "") || (kind != "user" && kind != "feedback")) {
			return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "global memory is only for cross-project user preferences or feedback; save project facts without global, inside a project")
		}
		if id == "" {
			if generatedID == "" {
				generatedID = newUUIDv4()
			}
			id = generatedID
		}
		entry := memoryEntry{
			Summary: memorySummary{
				ID: id, Project: project, Title: title, Kind: kind,
				Revision: expRev, UpdatedAtMs: 0, Archived: archivedFlag,
			},
			Body: bodyText,
		}
		if (!hasID || idVal == nil) && attempt == 0 {
			similar, err := s.findSimilar(ctx, entry)
			if err != nil {
				return nil, nil, nil, err
			}
			if len(similar) > 0 {
				s.mu.Lock()
				existing := retrieveEntries(similar, &s.remaining)
				s.mu.Unlock()
				return nil, nil, map[string]any{
					"status":   "similar",
					"saved":    false,
					"existing": existing,
					"hint":     "Similar memories exist. Update one with its id and revision, or save a distinct fact",
				}, nil
			}
		}
		ops, err := s.storage.putOperations(ctx, entry, expRev, s.project)
		if err != nil {
			return nil, nil, nil, err
		}
		action := "updated"
		if expRev == 0 {
			action = "created"
		}
		return ops, captureReceipt(ops, entry, action, s.settings.ContextBytes), nil, nil
	})
}

func (s *turnState) execForget(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
	return s.runWriter(ctx, func(_ int) ([]protocol.PluginTransactionOperation, *receiptSnapshot, map[string]any, error) {
		args, err := decodeArgs(call.Arguments, []string{"id", "expected_revision"}, []string{"id", "expected_revision"})
		if err != nil {
			return nil, nil, nil, err
		}
		id := strings.ToLower(args["id"].(string))
		for attempt := 0; ; attempt++ {
			cur, err := s.storage.readMemory(ctx, id, s.project)
			if err != nil {
				var fault *protocol.Fault
				if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
					return nil, nil, map[string]any{"status": "deleted", "saved": false, "deleted": []string{id}}, nil
				}
				return nil, nil, nil, err
			}
			ops, err := s.storage.removeOperations(ctx, id, cur.Summary.Revision, s.project)
			if err != nil {
				var rc recordChangedError
				if errors.As(err, &rc) && attempt < 2 {
					continue
				}
				return nil, nil, nil, err
			}
			return ops, captureReceipt(ops, cur, "deleted", s.settings.ContextBytes), nil, nil
		}
	})
}

func (s *turnState) execConsolidate(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
	return s.runWriter(ctx, func(_ int) ([]protocol.PluginTransactionOperation, *receiptSnapshot, map[string]any, error) {
		args, err := decodeArgs(call.Arguments, []string{"id", "expected_revision", "title", "body", "sources"}, []string{"id", "expected_revision", "title", "body", "sources"})
		if err != nil {
			return nil, nil, nil, err
		}
		rawSources, ok := args["sources"].([]any)
		if !ok || len(rawSources) == 0 {
			return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid consolidation arguments")
		}
		targetID := strings.ToLower(args["id"].(string))
		targetRev, _ := safeInt(args["expected_revision"])
		sources := make([]mergeSourceRef, 0, len(rawSources))
		expected := []mergeSourceRef{{ID: targetID, Revision: targetRev}}
		for _, item := range rawSources {
			m, ok := item.(map[string]any)
			if !ok {
				return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid consolidation arguments")
			}
			for k := range m {
				if k != "id" && k != "revision" {
					return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid consolidation arguments")
				}
			}
			sid, ok := m["id"].(string)
			rev, revOK := safeInt(m["revision"])
			if !ok || !revOK || rev < 1 {
				return nil, nil, nil, failFault(protocol.ErrorCodeInvalidRequest, "invalid consolidation arguments")
			}
			ref := mergeSourceRef{ID: strings.ToLower(sid), Revision: rev}
			sources = append(sources, ref)
			expected = append(expected, ref)
		}
		s.mu.Lock()
		reads := slices.Clone(s.reads)
		s.mu.Unlock()
		allIDs := make([]string, 0, len(expected))
		for _, ref := range expected {
			if err := hasReadAuthority(reads, ref.ID, ref.Revision); err != nil {
				return nil, nil, nil, err
			}
			allIDs = append(allIDs, ref.ID)
		}
		current := make([]memoryEntry, 0, len(expected))
		for _, ref := range expected {
			entry, err := s.storage.readMemory(ctx, ref.ID, s.project)
			if err != nil {
				var fault *protocol.Fault
				if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
					return nil, nil, nil, recordChangedError{ids: allIDs}
				}
				return nil, nil, nil, err
			}
			current = append(current, entry)
		}
		for i, entry := range current {
			if entry.Summary.Revision != expected[i].Revision || entry.Summary.Archived {
				return nil, nil, nil, recordChangedError{ids: allIDs}
			}
		}
		entry := current[0]
		entry.Summary.Title = args["title"].(string)
		entry.Body = args["body"].(string)
		ops, err := s.storage.mergeOperations(ctx, entry, sources, s.project)
		if err != nil {
			var rc recordChangedError
			if errors.As(err, &rc) {
				return nil, nil, nil, recordChangedError{ids: allIDs}
			}
			return nil, nil, nil, err
		}
		return ops, captureReceipt(ops, entry, "merged", s.settings.ContextBytes), nil, nil
	})
}
