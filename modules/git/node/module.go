// Package gitnode provides the compile-time native Node Git inspection agent
// tools paired with modules/git/desktop.
package gitnode

import (
	"context"
	_ "embed"
	"encoding/json"
	"math"
	"slices"
	"strings"
	"sync"

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

// Module returns the native Node module providing read-only Git agent tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "git" }
func (module) ReplacesPlugins() []string { return []string{"git"} }

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

func (module) ToolsForTurn(_ context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		tool, err := host.WorkspaceTool(inv, moduleapi.WorkspaceToolConfig{
			Spec:        spec,
			Instruction: catalog.Instructions,
			PrepareArgs: prepareFor(spec.Name),
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

const maxSafeInt = (1 << 53) - 1

func invalid(message string) error {
	if message == "" {
		message = "Invalid Git arguments"
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func decodeObject(raw json.RawMessage, allowed []string) (map[string]any, error) {
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, invalid("")
	}
	for k := range obj {
		if !slices.Contains(allowed, k) {
			return nil, invalid("")
		}
	}
	return obj, nil
}

func optionalString(obj map[string]any, key string) (any, bool) {
	v, exists := obj[key]
	if !exists || v == nil {
		return nil, true
	}
	s, ok := v.(string)
	return s, ok
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

func isHex40(s string) bool {
	if len(s) != 40 {
		return false
	}
	for i := 0; i < 40; i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return true
}

func checkPage(args map[string]any, defaultLimit int64) (int64, int64, error) {
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return 0, 0, invalid("offset must be a non-negative safe integer")
		}
		offset = n
	}
	limit := defaultLimit
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return 0, 0, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	if offset > maxSafeInt-limit {
		return 0, 0, invalid("offset + limit must be a safe integer")
	}
	return offset, limit, nil
}

func prepareFor(name string) func(context.Context, json.RawMessage) (json.RawMessage, error) {
	switch name {
	case "git_status":
		return prepareStatus
	case "git_diff":
		return prepareDiff
	case "git_log":
		return prepareLog
	case "git_show":
		return prepareShow
	case "git_blame":
		return prepareBlame
	case "list_git_changes":
		return prepareChanges
	case "read_git_change":
		return prepareChange
	default:
		return nil
	}
}

func prepareStatus(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	return json.Marshal(map[string]any{"worktree": wt})
}

func prepareDiff(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "path", "scope"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	scope := "all"
	if v, exists := args["scope"]; exists {
		s, ok := v.(string)
		if !ok || (s != "all" && s != "staged" && s != "unstaged") {
			return nil, invalid("")
		}
		scope = s
	}
	return json.Marshal(map[string]any{"worktree": wt, "path": path, "scope": scope})
}

func prepareLog(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "limit", "cursor", "revision", "path"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	rev, ok := optionalString(args, "revision")
	if !ok {
		return nil, invalid("")
	}
	path, ok := optionalString(args, "path")
	if !ok {
		return nil, invalid("")
	}
	limit := int64(20)
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return nil, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	var cursor any
	if v, ok := args["cursor"]; ok && v != nil {
		c, ok := v.(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		for k := range c {
			if k != "head" && k != "after" && k != "offset" && k != "path" {
				return nil, invalid("")
			}
		}
		if _, ok := optionalString(c, "path"); !ok {
			return nil, invalid("")
		}
		if off, ok := safeInt(c["offset"]); !ok || off < 0 {
			return nil, invalid("")
		}
		head, ok := c["head"].(string)
		if !ok || !isHex40(head) {
			return nil, invalid("")
		}
		after, ok := c["after"].(string)
		if !ok || !isHex40(after) {
			return nil, invalid("")
		}
		cursor = c
	}
	return json.Marshal(map[string]any{"worktree": wt, "limit": limit, "cursor": cursor, "revision": rev, "path": path})
}

func prepareShow(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "revision", "path", "offset", "limit"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	rev := "HEAD"
	if v, exists := args["revision"]; exists {
		s, ok := v.(string)
		if !ok {
			return nil, invalid("")
		}
		rev = s
	}
	path, ok := optionalString(args, "path")
	if !ok {
		return nil, invalid("")
	}
	offset, limit, err := checkPage(args, 2000)
	if err != nil {
		return nil, err
	}
	if path == nil && offset != 0 {
		return nil, invalid("")
	}
	return json.Marshal(map[string]any{"worktree": wt, "revision": rev, "path": path, "offset": offset, "limit": limit})
}

func prepareBlame(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "revision", "path", "offset", "limit"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	rev, ok := optionalString(args, "revision")
	if !ok {
		return nil, invalid("")
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	offset, limit, err := checkPage(args, 200)
	if err != nil {
		return nil, err
	}
	return json.Marshal(map[string]any{"worktree": wt, "revision": rev, "path": path, "offset": offset, "limit": limit})
}

func prepareChanges(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "target", "cursor", "limit"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	_, limit, err := checkPage(args, 200)
	if err != nil {
		return nil, err
	}
	cursor := args["cursor"]
	selected, hasTarget := args["target"]
	if !hasTarget {
		if cursor == nil {
			selected = map[string]any{"kind": "worktree", "data": map[string]any{"scope": "all"}}
		} else {
			selected = nil
		}
	}
	if (selected == nil) == (cursor == nil) {
		return nil, invalid("")
	}
	if selected != nil {
		m, ok := selected.(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		for k := range m {
			if k != "kind" && k != "data" {
				return nil, invalid("")
			}
		}
	}
	if cursor != nil {
		c, ok := cursor.(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		for k := range c {
			if k != "snapshot" && k != "after_path" {
				return nil, invalid("")
			}
		}
		snap, ok := c["snapshot"].(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		for k := range snap {
			if k != "target" && k != "metadata_revision" {
				return nil, invalid("")
			}
		}
		if _, ok := optionalString(c, "after_path"); !ok {
			return nil, invalid("")
		}
	}
	return json.Marshal(map[string]any{"worktree": wt, "target": selected, "cursor": cursor, "limit": limit})
}

func prepareChange(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"worktree", "snapshot", "path", "expected_metadata_revision", "expected_revision", "offset", "limit"})
	if err != nil {
		return nil, err
	}
	wt, ok := optionalString(args, "worktree")
	if !ok {
		return nil, invalid("")
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	snap, ok := args["snapshot"].(map[string]any)
	if !ok {
		return nil, invalid("")
	}
	for k := range snap {
		if k != "target" && k != "metadata_revision" {
			return nil, invalid("")
		}
	}
	metaRev, ok := args["expected_metadata_revision"].(string)
	if !ok {
		return nil, invalid("")
	}
	expRev, ok := optionalString(args, "expected_revision")
	if !ok {
		return nil, invalid("")
	}
	offset, limit, err := checkPage(args, 200)
	if err != nil {
		return nil, err
	}
	if offset > 0 && expRev == nil {
		return nil, invalid("")
	}
	return json.Marshal(map[string]any{
		"worktree": wt, "snapshot": snap, "path": path,
		"expected_metadata_revision": metaRev, "expected_revision": expRev,
		"offset": offset, "limit": limit,
	})
}
