// Package filesnode provides the compile-time native Node file, office and
// notebook agent tools. Confined file access, approval, checkpoints and
// revisions remain owned by the Node.
package filesnode

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

// Module returns the native Node module providing workspace file, office and
// notebook tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "files" }
func (module) ReplacesPlugins() []string { return []string{"files"} }

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
		prepare := prepareFor(spec.Name)
		tool, err := host.WorkspaceTool(inv, moduleapi.WorkspaceToolConfig{
			Spec:        spec,
			Instruction: catalog.Instructions,
			PrepareArgs: prepare,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func invalid(message string) error {
	if message == "" {
		message = "Invalid file tool arguments"
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func decodeObject(raw json.RawMessage, allowed []string, errMsg string) (map[string]any, error) {
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, invalid(errMsg)
	}
	for k := range obj {
		if !slices.Contains(allowed, k) {
			return nil, invalid(errMsg)
		}
	}
	return obj, nil
}

func isHex64(s string) bool {
	if len(s) != 64 {
		return false
	}
	for i := 0; i < 64; i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return true
}

// optionalRevision validates a supplied revision. An omitted revision is left
// for the Node, which resolves it from this conversation's observations.
func optionalRevision(args map[string]any) (string, error) {
	v, ok := args["expected_revision"]
	if !ok {
		return "", nil
	}
	rev, ok := v.(string)
	if !ok || !isHex64(rev) {
		return "", invalid("expected_revision must be the 64-character revision from read_file")
	}
	return rev, nil
}

func withRevision(out map[string]any, rev string) map[string]any {
	if rev != "" {
		out["expected_revision"] = rev
	}
	return out
}

func safeInt(v any) (int64, bool) {
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil {
			const maxSafe = (1 << 53) - 1
			if i >= -maxSafe && i <= maxSafe {
				return i, true
			}
		}
		if f, err := n.Float64(); err == nil && !math.IsNaN(f) && !math.IsInf(f, 0) && math.Trunc(f) == f {
			const maxSafe = (1 << 53) - 1
			if f >= -maxSafe && f <= maxSafe {
				return int64(f), true
			}
		}
	case float64:
		const maxSafe = (1 << 53) - 1
		if !math.IsNaN(n) && !math.IsInf(n, 0) && math.Trunc(n) == n && n >= -maxSafe && n <= maxSafe {
			return int64(n), true
		}
	}
	return 0, false
}

func prepareFor(name string) func(context.Context, json.RawMessage) (json.RawMessage, error) {
	switch name {
	case "list_directory":
		return prepareList
	case "read_file":
		return prepareRead
	case "write_file":
		return prepareWrite
	case "edit_file":
		return prepareEdit
	case "apply_patch":
		return prepareApply
	case "delete_file":
		return prepareDelete
	case "move_file":
		return prepareMove
	case "create_directory":
		return prepareMkdir
	case "find_files":
		return prepareFind
	case "search_files":
		return prepareSearch
	case "read_office":
		return prepareOffice
	case "read_notebook":
		return prepareNotebookRead
	case "edit_notebook":
		return prepareNotebookEdit
	default:
		return nil
	}
}

func prepareList(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "cursor"}, "")
	if err != nil {
		return nil, err
	}
	path := ""
	if v, ok := args["path"]; ok {
		s, ok := v.(string)
		if !ok {
			return nil, invalid("")
		}
		path = s
	}
	var cursor any
	if v, ok := args["cursor"]; ok && v != nil {
		c, ok := v.(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		for k := range c {
			if k != "revision" && k != "directory" && k != "name" {
				return nil, invalid("")
			}
		}
		if _, ok := c["revision"].(string); !ok {
			return nil, invalid("")
		}
		if _, ok := c["directory"].(bool); !ok {
			return nil, invalid("")
		}
		if _, ok := c["name"].(string); !ok {
			return nil, invalid("")
		}
		cursor = c
	}
	if path == "." {
		path = ""
	}
	return json.Marshal(map[string]any{"path": path, "after": cursor})
}

func prepareRead(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "offset", "limit"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("offset must be a non-negative safe integer")
		}
		offset = n
	}
	limit := int64(2000)
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return nil, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	return json.Marshal(map[string]any{"path": path, "offset": offset, "limit": limit})
}

func prepareWrite(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "text", "expected_revision"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	text, ok := args["text"].(string)
	if !ok {
		return nil, invalid("")
	}
	out := map[string]any{"path": path, "text": text}
	if v, ok := args["expected_revision"]; ok {
		// Explicit null creates; an omitted revision is resolved by the Node.
		if _, ok := v.(string); !ok && v != nil {
			return nil, invalid("expected_revision must be a string or null")
		}
		out["expected_revision"] = v
	}
	return json.Marshal(out)
}

func prepareEdit(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "old_text", "new_text", "replace_all", "edits", "expected_revision"}, "Expected edit arguments with supported fields")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("path must be a string")
	}
	rev, err := optionalRevision(args)
	if err != nil {
		return nil, err
	}
	if editsVal, hasEdits := args["edits"]; hasEdits {
		if _, hasOld := args["old_text"]; hasOld {
			return nil, invalid("Use either edits or old_text and new_text")
		}
		if _, hasNew := args["new_text"]; hasNew {
			return nil, invalid("Use either edits or old_text and new_text")
		}
		if ra, hasRA := args["replace_all"]; hasRA && ra != false {
			return nil, invalid("Set replace_all on each edits entry")
		}
		list, ok := editsVal.([]any)
		if !ok || len(list) == 0 {
			return nil, invalid("edits must be a nonempty array")
		}
		for _, item := range list {
			entry, ok := item.(map[string]any)
			if !ok {
				return nil, invalid("edits entry must contain supported edit fields")
			}
			for k := range entry {
				if k != "old_text" && k != "new_text" && k != "replace_all" {
					return nil, invalid("edits entry must contain supported edit fields")
				}
			}
			oldText, ok := entry["old_text"].(string)
			if !ok || oldText == "" {
				return nil, invalid("edits.old_text must be a nonempty string")
			}
			if _, ok := entry["new_text"].(string); !ok {
				return nil, invalid("edits.new_text must be a string")
			}
			if ra, hasRA := entry["replace_all"]; hasRA {
				if _, ok := ra.(bool); !ok {
					return nil, invalid("edits.replace_all must be a boolean")
				}
			}
		}
		return json.Marshal(withRevision(map[string]any{"path": path, "edits": list}, rev))
	}
	oldText, ok := args["old_text"].(string)
	if !ok || oldText == "" {
		return nil, invalid("old_text must be a nonempty string")
	}
	newText, ok := args["new_text"].(string)
	if !ok {
		return nil, invalid("new_text must be a string")
	}
	out := withRevision(map[string]any{"path": path, "old_text": oldText, "new_text": newText}, rev)
	if ra, hasRA := args["replace_all"]; hasRA {
		b, ok := ra.(bool)
		if !ok {
			return nil, invalid("replace_all must be a boolean")
		}
		out["replace_all"] = b
	}
	return json.Marshal(out)
}

func prepareApply(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"input"}, "input must be the whole patch text")
	if err != nil {
		return nil, err
	}
	input, ok := args["input"].(string)
	if !ok || input == "" {
		return nil, invalid("input must be the whole patch text")
	}
	return json.Marshal(map[string]any{"input": input})
}

func prepareDelete(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "expected_revision"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	rev, err := optionalRevision(args)
	if err != nil {
		return nil, err
	}
	return json.Marshal(withRevision(map[string]any{"path": path}, rev))
}

func prepareMove(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "destination", "expected_revision"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	dst, ok := args["destination"].(string)
	if !ok {
		return nil, invalid("")
	}
	rev, err := optionalRevision(args)
	if err != nil {
		return nil, err
	}
	return json.Marshal(withRevision(map[string]any{"path": path, "destination": dst}, rev))
}

func prepareMkdir(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	return json.Marshal(map[string]any{"path": path})
}

func prepareFind(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"globs", "path", "limit", "offset"}, "")
	if err != nil {
		return nil, err
	}
	rawGlobs, ok := args["globs"].([]any)
	if !ok || len(rawGlobs) == 0 {
		return nil, invalid("")
	}
	globs := make([]string, 0, len(rawGlobs))
	for _, item := range rawGlobs {
		s, ok := item.(string)
		if !ok || s == "" {
			return nil, invalid("")
		}
		globs = append(globs, s)
	}
	path := ""
	if v, ok := args["path"]; ok {
		s, ok := v.(string)
		if !ok {
			return nil, invalid("path must be a string")
		}
		path = s
	}
	if path == "." {
		path = ""
	}
	limit := int64(500)
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return nil, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("offset must be a non-negative safe integer")
		}
		offset = n
	}
	return json.Marshal(map[string]any{"globs": globs, "path": path, "limit": limit, "offset": offset})
}

func prepareSearch(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"query", "regex", "case_sensitive", "globs", "path", "mode", "context", "type", "limit", "offset", "multiline"}, "Expected search arguments with supported fields")
	if err != nil {
		return nil, err
	}
	query, ok := args["query"].(string)
	if !ok || query == "" {
		return nil, invalid("query must be a nonempty string")
	}
	regex := false
	if v, ok := args["regex"]; ok {
		b, ok := v.(bool)
		if !ok {
			return nil, invalid("")
		}
		regex = b
	}
	caseSensitive := false
	if v, ok := args["case_sensitive"]; ok {
		b, ok := v.(bool)
		if !ok {
			return nil, invalid("")
		}
		caseSensitive = b
	}
	globs := []string{}
	if v, ok := args["globs"]; ok {
		list, ok := v.([]any)
		if !ok {
			return nil, invalid("")
		}
		for _, item := range list {
			s, ok := item.(string)
			if !ok {
				return nil, invalid("")
			}
			globs = append(globs, s)
		}
	}
	path := ""
	if v, ok := args["path"]; ok {
		s, ok := v.(string)
		if !ok {
			return nil, invalid("")
		}
		path = s
	}
	if path == "." {
		path = ""
	}
	mode := "files"
	if v, ok := args["mode"]; ok {
		s, ok := v.(string)
		if !ok || (s != "content" && s != "files" && s != "count") {
			return nil, invalid("")
		}
		mode = s
	}
	ctxLines := int64(0)
	if v, ok := args["context"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("context must be a non-negative safe integer")
		}
		ctxLines = n
	}
	var fileType any
	if v, ok := args["type"]; ok && v != nil {
		s, ok := v.(string)
		if !ok {
			return nil, invalid("")
		}
		fileType = s
	}
	limit := int64(100)
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return nil, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("offset must be a non-negative safe integer")
		}
		offset = n
	}
	multiline := false
	if v, ok := args["multiline"]; ok {
		b, ok := v.(bool)
		if !ok {
			return nil, invalid("")
		}
		multiline = b
	}
	return json.Marshal(map[string]any{
		"query": query, "regex": regex, "case_sensitive": caseSensitive, "globs": globs,
		"path": path, "mode": mode, "context": ctxLines, "type": fileType,
		"limit": limit, "offset": offset, "multiline": multiline,
	})
}

func prepareOffice(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"source", "offset"}, "")
	if err != nil {
		return nil, err
	}
	src, ok := args["source"].(map[string]any)
	if !ok {
		return nil, invalid("")
	}
	for k := range src {
		if k != "kind" && k != "value" {
			return nil, invalid("")
		}
	}
	kind, ok := src["kind"].(string)
	if !ok || (kind != "path" && kind != "attachment") {
		return nil, invalid("")
	}
	val, ok := src["value"].(string)
	if !ok || val == "" {
		return nil, invalid("")
	}
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("")
		}
		offset = n
	}
	return json.Marshal(map[string]any{"source": map[string]any{"kind": kind, "value": val}, "offset": offset})
}

func prepareNotebookRead(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "offset", "limit"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	offset := int64(0)
	if v, ok := args["offset"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 0 {
			return nil, invalid("offset must be a non-negative safe integer")
		}
		offset = n
	}
	limit := int64(20)
	if v, ok := args["limit"]; ok {
		n, ok := safeInt(v)
		if !ok || n < 1 {
			return nil, invalid("limit must be a positive safe integer")
		}
		limit = n
	}
	return json.Marshal(map[string]any{"path": path, "offset": offset, "limit": limit})
}

func prepareNotebookEdit(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw, []string{"path", "expected_revision", "edits"}, "")
	if err != nil {
		return nil, err
	}
	path, ok := args["path"].(string)
	if !ok {
		return nil, invalid("")
	}
	rev, err := optionalRevision(args)
	if err != nil {
		return nil, err
	}
	edits, ok := args["edits"].([]any)
	if !ok || len(edits) == 0 {
		return nil, invalid("")
	}
	for _, item := range edits {
		entry, ok := item.(map[string]any)
		if !ok {
			return nil, invalid("")
		}
		kind, ok := entry["kind"].(string)
		if !ok || (kind != "replace" && kind != "insert" && kind != "delete") {
			return nil, invalid("")
		}
		var allowed []string
		switch kind {
		case "insert":
			allowed = []string{"kind", "index", "cell_type", "source"}
		case "replace":
			allowed = []string{"kind", "index", "source"}
		default:
			allowed = []string{"kind", "index"}
		}
		for k := range entry {
			if !slices.Contains(allowed, k) {
				return nil, invalid("")
			}
		}
		idx, ok := safeInt(entry["index"])
		if !ok || idx < 0 {
			return nil, invalid("")
		}
		if kind != "delete" {
			src, ok := entry["source"].(string)
			if !ok || strings.ContainsRune(src, 0) {
				return nil, invalid("")
			}
		}
		if kind == "insert" {
			ct, ok := entry["cell_type"].(string)
			if !ok || (ct != "code" && ct != "markdown" && ct != "raw") {
				return nil, invalid("")
			}
		}
	}
	return json.Marshal(withRevision(map[string]any{"path": path, "edits": edits}, rev))
}
