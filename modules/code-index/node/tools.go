package codeindex

import (
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"
	"strconv"
	"strings"
	"sync"

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

// Module returns the compile-time native Node module providing the six code
// navigation and chained-read tools directly in Go without JS flow round-trips.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "code-index" }
func (module) ReplacesPlugins() []string { return []string{"code"} }

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
	hasIndex := host.HasCodeIndex()
	instruction := catalog.Instructions
	if !hasIndex {
		instruction = ""
	}
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		switch spec.Name {
		case "code_search", "code_outline":
			if !hasIndex {
				continue
			}
			tool, err := host.WorkspaceTool(inv, moduleapi.WorkspaceToolConfig{
				Spec:        spec,
				Instruction: instruction,
			})
			if err != nil {
				return nil, err
			}
			tools = append(tools, tool)
		case "code_context":
			if !hasIndex {
				continue
			}
			spec.ReadOnly, spec.Concurrent = true, true
			spec.StreamRead = inv.Worktree.Project != nil
			tool, err := moduleapi.BuildCustomTool(spec, instruction, func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
				return runCodeContext(ctx, host, inv, call.Arguments)
			})
			if err != nil {
				return nil, err
			}
			tool.Flow = []protocol.ToolOperation{protocol.ToolOperationSearchCode, protocol.ToolOperationGraphCode, protocol.ToolOperationReadFileLines}
			tools = append(tools, tool)
		case "code_impact":
			if !hasIndex {
				continue
			}
			spec.ReadOnly, spec.Concurrent = true, true
			spec.StreamRead = inv.Worktree.Project != nil
			tool, err := moduleapi.BuildCustomTool(spec, instruction, func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
				return runCodeImpact(ctx, host, inv, call.Arguments)
			})
			if err != nil {
				return nil, err
			}
			tool.Flow = []protocol.ToolOperation{protocol.ToolOperationGraphCode}
			tools = append(tools, tool)
		case "code_map":
			if !hasIndex {
				continue
			}
			spec.ReadOnly, spec.Concurrent = true, true
			spec.StreamRead = inv.Worktree.Project != nil
			tool, err := moduleapi.BuildCustomTool(spec, instruction, func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
				return runCodeMap(ctx, host, inv, call.Arguments)
			})
			if err != nil {
				return nil, err
			}
			tool.Flow = []protocol.ToolOperation{protocol.ToolOperationGraphCode}
			tools = append(tools, tool)
		case "code_pipeline":
			if !hasIndex {
				var ok bool
				spec, ok = fileOnlyPipelineSpec(spec)
				if !ok {
					continue
				}
			}
			spec.ReadOnly, spec.Concurrent = true, true
			spec.StreamRead = inv.Worktree.Project != nil
			tool, err := moduleapi.BuildCustomTool(spec, instruction, func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
				return runCodePipeline(ctx, host, inv, hasIndex, call.Arguments)
			})
			if err != nil {
				return nil, err
			}
			if hasIndex {
				tool.Flow = []protocol.ToolOperation{protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles, protocol.ToolOperationReadFileLines, protocol.ToolOperationSearchCode, protocol.ToolOperationOutlineCode, protocol.ToolOperationGraphCode}
			} else {
				tool.Flow = []protocol.ToolOperation{protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles, protocol.ToolOperationReadFileLines}
			}
			tools = append(tools, tool)
		}
	}
	return tools, nil
}

func fileOnlyPipelineSpec(spec moduleapi.ToolSpec) (moduleapi.ToolSpec, bool) {
	var schema map[string]any
	if json.Unmarshal(spec.Parameters, &schema) != nil {
		return spec, false
	}
	properties, _ := schema["properties"].(map[string]any)
	steps, _ := properties["steps"].(map[string]any)
	items, _ := steps["items"].(map[string]any)
	stepProps, _ := items["properties"].(map[string]any)
	op, ok := stepProps["operation"].(map[string]any)
	if !ok {
		return spec, false
	}
	op["enum"] = []string{"files.find", "files.search", "files.read_lines"}
	encoded, err := json.Marshal(schema)
	if err != nil {
		return spec, false
	}
	spec.Parameters = encoded
	spec.Description = "Chain dependent file reads and return only the selected final data (16 KiB). Use from_previous to bind at least one argument per follow-up, such as the first path from files.find to files.read_lines. Use ordinary calls for independent reads."
	return spec, true
}

const (
	maxContextSymbols  = 20
	maxContextSnippets = 5
	maxContextLines    = 80
	maxCodeBytes       = 16 * 1024
	graphItemsLimit    = 8
	impactFilesLimit   = 5
	impactSymbolsLimit = 3
	reasonCharsLimit   = 160
	maxToolGraphItems  = 50
	defaultGraphItems  = 20
	maxToolGraphPaths  = 64
	maxImpactTargets   = 10
)

func invalidResult(message string) engine.ToolResult {
	return engine.ToolResult{
		Value:         map[string]any{"isError": true, "error": map[string]any{"code": "invalid_request", "message": message}},
		ReturnedError: true,
	}
}

func faultResult(err error) engine.ToolResult {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return engine.ToolResult{Failure: fault}
	}
	return engine.ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: err.Error()}}
}

func jsonSize(v any) int {
	b, err := json.Marshal(v)
	if err != nil {
		return 0
	}
	return len(b)
}

func safeInt(v any) (int64, bool) {
	const maxSafe = (1 << 53) - 1
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil && i >= -maxSafe && i <= maxSafe {
			return i, true
		}
		if f, err := n.Float64(); err == nil && !math.IsNaN(f) && !math.IsInf(f, 0) && math.Trunc(f) == f && f >= -maxSafe && f <= maxSafe {
			return int64(f), true
		}
	case float64:
		if !math.IsNaN(n) && !math.IsInf(n, 0) && math.Trunc(n) == n && n >= -maxSafe && n <= maxSafe {
			return int64(n), true
		}
	case int:
		return int64(n), true
	case int64:
		return n, true
	}
	return 0, false
}

func boundInt(input map[string]any, key string, fallback, maximum, minimum int) (int, error) {
	v, exists := input[key]
	if !exists {
		return fallback, nil
	}
	n, ok := safeInt(v)
	if !ok || n < int64(minimum) || n > int64(maximum) {
		return 0, errors.New("Invalid code tool limit")
	}
	return int(n), nil
}

func decodeMap(raw json.RawMessage, label string) (map[string]any, error) {
	if len(raw) == 0 {
		return map[string]any{}, nil
	}
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, fmt.Errorf("Invalid %s query", label)
	}
	return obj, nil
}

func runOp(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation, op protocol.ToolOperation, args any) (protocol.Output, error) {
	encoded, err := json.Marshal(args)
	if err != nil {
		return protocol.Output{}, err
	}
	return host.RunWorkspaceOperation(ctx, inv, op, encoded)
}

func compactSymbol(sym protocol.CodeIndexSymbol) map[string]any {
	start := sym.Range.StartLine + 1
	end := sym.Range.EndLine
	if sym.Range.EndColumn > 0 {
		end++
	}
	if end < start {
		end = start
	}
	item := map[string]any{
		"path": sym.Path, "language": sym.Language, "name": sym.Name,
		"kind": sym.Kind, "depth": sym.Depth, "start_line": start, "end_line": end,
	}
	if sym.Signature != nil {
		item["signature"] = *sym.Signature
	}
	return item
}

func graphDefinition(t protocol.CodeIndexGraphDefinition) map[string]any {
	return map[string]any{
		"path": t.Path, "name": t.Name, "kind": t.Kind,
		"start_line": t.StartLine + 1, "end_line": t.EndLine + 1,
	}
}

func graphCaller(r protocol.CodeIndexGraphReference) map[string]any {
	item := map[string]any{
		"path": r.Path, "line": r.Line + 1, "kind": r.Kind, "confidence": r.Confidence,
	}
	if r.Scope != nil {
		item["scope"] = r.Scope.Name
	}
	if r.Count > 1 {
		item["count"] = r.Count
	}
	if r.Ambiguous {
		item["ambiguous"] = true
	}
	return item
}

func graphCallee(r protocol.CodeIndexGraphReference) map[string]any {
	item := map[string]any{
		"name": r.Name, "line": r.Line + 1, "kind": r.Kind, "confidence": r.Confidence,
	}
	if r.Count > 1 {
		item["count"] = r.Count
	}
	if r.Ambiguous {
		item["ambiguous"] = true
		item["candidates"] = r.Candidates
	} else if len(r.Targets) > 0 {
		item["definition"] = map[string]any{"path": r.Targets[0].Path, "line": r.Targets[0].StartLine + 1}
	}
	return item
}

func graphImpacted(file protocol.CodeIndexGraphImpacted, symbolLimit int) map[string]any {
	item := map[string]any{
		"path": file.Path, "depth": file.Depth, "confidence": file.Confidence,
	}
	if file.Ambiguous {
		item["ambiguous"] = true
	}
	limit := len(file.Symbols)
	if symbolLimit >= 0 && limit > symbolLimit {
		limit = symbolLimit
	}
	syms := make([]map[string]any, 0, limit)
	for _, s := range file.Symbols[:limit] {
		aff := map[string]any{
			"name": s.Name, "kind": s.Kind, "start_line": s.StartLine + 1, "depth": s.Depth,
		}
		if s.Confidence != file.Confidence {
			aff["confidence"] = s.Confidence
		}
		if s.Ambiguous {
			aff["ambiguous"] = true
		}
		syms = append(syms, aff)
	}
	item["symbols"] = syms
	if file.Truncated || len(file.Symbols) > len(syms) {
		item["truncated"] = true
	}
	return item
}

func graphRanked(s protocol.CodeIndexGraphRanked) map[string]any {
	item := map[string]any{
		"name": s.Name, "kind": s.Kind, "start_line": s.StartLine + 1, "end_line": s.EndLine + 1,
	}
	if s.Signature != nil {
		item["signature"] = *s.Signature
	}
	if s.References > 0 {
		item["references"] = s.References
	}
	return item
}

func graphStatus(page protocol.CodeIndexGraphGraphPage) map[string]any {
	item := map[string]any{
		"total": page.Total, "index_revision": page.IndexRevision,
		"partial": page.Partial, "truncated": page.Truncated,
	}
	if page.NextOffset != nil {
		item["next_offset"] = *page.NextOffset
	}
	if len(page.Issues) > 0 {
		item["skipped"] = page.Skipped
		issues := page.Issues
		if len(issues) > 10 {
			issues = issues[:10]
		}
		item["issues"] = issues
	}
	return item
}

func fitGraphList(data map[string]any, key string, budget, offset int) (map[string]any, error) {
	if jsonSize(data) <= budget {
		return data, nil
	}
	items, _ := data[key].([]map[string]any)
	// Trim issues and targets before dropping list items.
	if issues, ok := data["issues"].([]protocol.CodeIndexIssue); ok {
		for jsonSize(data) > budget && len(issues) > 0 {
			issues = issues[:len(issues)-1]
			data["issues"] = issues
			data["partial"], data["truncated"] = true, true
		}
	}
	if targets, ok := data["targets"].([]map[string]any); ok {
		for jsonSize(data) > budget && len(targets) > 0 {
			targets = targets[:len(targets)-1]
			data["targets"] = targets
			data["partial"], data["truncated"] = true, true
		}
	}
	for jsonSize(data) > budget && len(items) > 0 {
		items = items[:len(items)-1]
		data[key] = items
		data["partial"], data["truncated"] = true, true
		data["next_offset"] = offset + max(1, len(items))
	}
	if jsonSize(data) > budget {
		return nil, errors.New("Code result metadata exceeds the output budget")
	}
	return data, nil
}

type snippetRange struct {
	path      string
	revision  string
	start     int
	end       int
	limit     int
	truncated bool
}

func runCodeContext(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation, raw json.RawMessage) engine.ToolResult {
	input, err := decodeMap(raw, "code context")
	if err != nil {
		return invalidResult(err.Error())
	}
	limit, err := boundInt(input, "limit", maxContextSymbols, maxContextSymbols, 1)
	if err != nil {
		return invalidResult(err.Error())
	}
	maxSnippets, err := boundInt(input, "max_snippets", maxContextSnippets, maxContextSnippets, 1)
	if err != nil {
		return invalidResult(err.Error())
	}
	budget, err := boundInt(input, "max_bytes", maxCodeBytes, maxCodeBytes, 512)
	if err != nil {
		return invalidResult(err.Error())
	}
	query := map[string]any{"limit": limit}
	for _, key := range []string{"query", "name", "path", "language"} {
		if v, exists := input[key]; exists {
			s, ok := v.(string)
			if !ok {
				return invalidResult("Invalid code context filter")
			}
			query[key] = s
		}
	}
	out, err := runOp(ctx, host, inv, protocol.ToolOperationSearchCode, query)
	if err != nil {
		return faultResult(err)
	}
	page, ok := out.Data.(protocol.CodeIndexSearchPage)
	if !ok {
		return invalidResult("Invalid code search result")
	}
	rawSymbols := page.Symbols
	if len(rawSymbols) > limit {
		rawSymbols = rawSymbols[:limit]
	}
	issues := page.Issues
	if issues == nil {
		issues = []protocol.CodeIndexIssue{}
	}
	if len(issues) > 10 {
		issues = issues[:10]
	}
	compactSymbols := make([]map[string]any, 0, len(rawSymbols))
	for _, s := range rawSymbols {
		compactSymbols = append(compactSymbols, compactSymbol(s))
	}
	snippets := []map[string]any{}
	data := map[string]any{
		"symbols":        compactSymbols,
		"snippets":       snippets,
		"index_revision": page.IndexRevision,
		"partial":        page.Partial,
		"truncated":      page.Truncated,
		"visited":        page.Visited,
		"skipped":        page.Skipped,
		"issues":         issues,
	}
	if len(page.Symbols) > len(rawSymbols) || len(page.Issues) > len(issues) {
		data["partial"], data["truncated"] = true, true
	}
	for jsonSize(data) > budget && len(issues) > 0 {
		issues = issues[:len(issues)-1]
		data["issues"] = issues
		data["partial"], data["truncated"] = true, true
	}
	for jsonSize(data) > budget && len(compactSymbols) > 0 {
		compactSymbols = compactSymbols[:len(compactSymbols)-1]
		rawSymbols = rawSymbols[:len(rawSymbols)-1]
		data["symbols"] = compactSymbols
		data["partial"], data["truncated"] = true, true
	}
	if jsonSize(data) > budget {
		return invalidResult("Code context metadata exceeds the output budget")
	}

	// Build merged source ranges around the selected symbols.
	type fileKey struct{ path, rev string }
	byFile := map[fileKey][]snippetRange{}
	var order []fileKey
	for _, s := range rawSymbols {
		fk := fileKey{s.Path, s.FileRevision}
		if _, exists := byFile[fk]; !exists {
			order = append(order, fk)
		}
		start := max(0, int(s.Range.StartLine)-2)
		endLine := int(s.Range.EndLine)
		if s.Range.EndColumn > 0 {
			endLine++
		}
		end := max(int(s.Range.StartLine)+1, endLine) + 2
		byFile[fk] = append(byFile[fk], snippetRange{path: s.Path, revision: s.FileRevision, start: start, end: end})
	}
	var merged []snippetRange
	for _, fk := range order {
		list := byFile[fk]
		slices.SortFunc(list, func(a, b snippetRange) int {
			if a.start != b.start {
				return a.start - b.start
			}
			return a.end - b.end
		})
		for _, r := range list {
			if len(merged) > 0 && merged[len(merged)-1].path == r.path && merged[len(merged)-1].revision == r.revision && r.start <= merged[len(merged)-1].end {
				if r.end > merged[len(merged)-1].end {
					merged[len(merged)-1].end = r.end
				}
			} else {
				merged = append(merged, r)
			}
		}
	}
	if len(merged) > maxSnippets {
		data["partial"], data["truncated"] = true, true
		merged = merged[:maxSnippets]
	}
	for i := range merged {
		length := merged[i].end - merged[i].start
		if length > maxContextLines {
			data["partial"], data["truncated"] = true, true
			merged[i].limit = maxContextLines
			merged[i].truncated = true
		} else {
			merged[i].limit = length
		}
	}

	// Optional graph neighborhood for the first symbol.
	if len(rawSymbols) > 0 {
		first := rawSymbols[0]
		graphOut, gErr := runOp(ctx, host, inv, protocol.ToolOperationGraphCode, map[string]any{
			"view": "context", "name": first.Name, "paths": []string{first.Path},
			"line": first.Range.StartLine, "limit": graphItemsLimit,
		})
		if gErr != nil {
			reason := []rune(gErr.Error())
			data["partial"] = true
			if len(reason) > reasonCharsLimit {
				data["graph_unavailable"] = string(reason[:reasonCharsLimit-1]) + "…"
			} else {
				data["graph_unavailable"] = string(reason)
			}
		} else if gp, ok := graphOut.Data.(protocol.CodeIndexGraphGraphPage); ok && gp.View == protocol.CodeIndexGraphViewContext {
			callers := gp.Callers
			if len(callers) > graphItemsLimit {
				callers = callers[:graphItemsLimit]
			}
			var callees []protocol.CodeIndexGraphReference
			for _, ref := range gp.Callees {
				if ref.Confidence != nil && *ref.Confidence != "" {
					callees = append(callees, ref)
					if len(callees) == graphItemsLimit {
						break
					}
				}
			}
			impFiles := gp.Impact
			if len(impFiles) > impactFilesLimit {
				impFiles = impFiles[:impactFilesLimit]
			}
			callerItems := make([]map[string]any, 0, len(callers))
			for _, c := range callers {
				callerItems = append(callerItems, graphCaller(c))
			}
			calleeItems := make([]map[string]any, 0, len(callees))
			for _, c := range callees {
				calleeItems = append(calleeItems, graphCallee(c))
			}
			impItems := make([]map[string]any, 0, len(impFiles))
			for _, f := range impFiles {
				impItems = append(impItems, graphImpacted(f, impactSymbolsLimit))
			}
			data["callers"] = callerItems
			data["callees"] = calleeItems
			data["impact"] = map[string]any{"total": gp.Total, "files": impItems}
			if gp.Partial {
				data["partial"] = true
			}
			if gp.Truncated || len(gp.Callers) > len(callers) || gp.Total > uint64(len(impFiles)) {
				data["truncated"] = true
			}
			for jsonSize(data)+128 > budget && len(callerItems) > 0 {
				callerItems = callerItems[:len(callerItems)-1]
				data["callers"] = callerItems
				data["partial"], data["truncated"] = true, true
			}
			for jsonSize(data)+128 > budget && len(calleeItems) > 0 {
				calleeItems = calleeItems[:len(calleeItems)-1]
				data["callees"] = calleeItems
				data["partial"], data["truncated"] = true, true
			}
			for jsonSize(data)+128 > budget && len(impItems) > 0 {
				impItems = impItems[:len(impItems)-1]
				data["impact"] = map[string]any{"total": gp.Total, "files": impItems}
				data["partial"], data["truncated"] = true, true
			}
		}
	}

	// Revision-checked source reads directly in Go.
	for idx, r := range merged {
		if jsonSize(data)+128 >= budget {
			if idx < len(merged) {
				data["partial"], data["truncated"] = true, true
			}
			break
		}
		readOut, rErr := runOp(ctx, host, inv, protocol.ToolOperationReadFileLines, map[string]any{
			"path": r.path, "offset": r.start, "limit": r.limit, "expected_revision": r.revision,
		})
		if rErr != nil {
			data["partial"] = true
			continue
		}
		linesPage, ok := readOut.Data.(protocol.FileLines)
		if !ok {
			data["partial"] = true
			continue
		}
		incomplete := linesPage.NextOffset != nil && *linesPage.NextOffset < uint64(r.start+r.limit)
		if incomplete {
			data["partial"], data["truncated"] = true, true
		}
		if len(linesPage.Lines) == 0 {
			data["partial"] = true
			continue
		}
		var sb strings.Builder
		for _, line := range linesPage.Lines {
			sb.WriteString(line.Text)
		}
		snippet := map[string]any{
			"path":          r.path,
			"file_revision": r.revision,
			"start_line":    linesPage.Lines[0].Number,
			"end_line":      linesPage.Lines[len(linesPage.Lines)-1].Number,
			"text":          sb.String(),
			"truncated":     r.truncated || incomplete,
		}
		snippets = append(snippets, snippet)
		data["snippets"] = snippets
		if jsonSize(data) > budget {
			data["partial"], data["truncated"] = true, true
			snippet["truncated"] = true
			runes := []rune(sb.String())
			lo, hi := 0, len(runes)
			for lo < hi {
				mid := (lo + hi + 1) / 2
				snippet["text"] = string(runes[:mid])
				if jsonSize(data) <= budget {
					lo = mid
				} else {
					hi = mid - 1
				}
			}
			trimmed := string(runes[:lo])
			snippet["text"] = trimmed
			if trimmed == "" || jsonSize(data) > budget {
				snippets = snippets[:len(snippets)-1]
				data["snippets"] = snippets
			} else {
				lineCount := strings.Count(trimmed, "\n")
				if !strings.HasSuffix(trimmed, "\n") {
					lineCount++
				}
				snippet["end_line"] = linesPage.Lines[0].Number + uint64(max(0, lineCount-1))
			}
		}
	}
	return engine.ToolResult{Value: data}
}

func parseGraphPaths(v any) ([]string, error) {
	if v == nil {
		return nil, nil
	}
	list, ok := v.([]any)
	if !ok || len(list) > maxToolGraphPaths {
		return nil, errors.New("Invalid code graph paths")
	}
	out := make([]string, 0, len(list))
	for _, item := range list {
		s, ok := item.(string)
		if !ok || s == "" {
			return nil, errors.New("Invalid code graph paths")
		}
		out = append(out, s)
	}
	return out, nil
}

func runCodeImpact(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation, raw json.RawMessage) engine.ToolResult {
	input, err := decodeMap(raw, "code impact")
	if err != nil {
		return invalidResult(err.Error())
	}
	_, hasPath := input["path"]
	_, hasPaths := input["paths"]
	if hasPath && hasPaths {
		return invalidResult("Use path or paths, not both")
	}
	var selected []string
	if hasPath {
		s, ok := input["path"].(string)
		if !ok || s == "" {
			return invalidResult("Invalid code graph paths")
		}
		selected = []string{s}
	} else if hasPaths {
		selected, err = parseGraphPaths(input["paths"])
		if err != nil {
			return invalidResult(err.Error())
		}
	}
	depth, err := boundInt(input, "depth", 3, 3, 1)
	if err != nil {
		return invalidResult(err.Error())
	}
	limit, err := boundInt(input, "limit", defaultGraphItems, maxToolGraphItems, 1)
	if err != nil {
		return invalidResult(err.Error())
	}
	offset, err := boundInt(input, "offset", 0, 10000, 0)
	if err != nil {
		return invalidResult(err.Error())
	}
	budget, err := boundInt(input, "max_bytes", maxCodeBytes, maxCodeBytes, 1024)
	if err != nil {
		return invalidResult(err.Error())
	}
	query := map[string]any{"view": "impact", "depth": depth, "limit": limit, "offset": offset}
	if v, exists := input["name"]; exists {
		s, ok := v.(string)
		if !ok || strings.TrimSpace(s) == "" {
			return invalidResult("Invalid code impact name")
		}
		query["name"] = s
	}
	if len(selected) > 0 {
		query["paths"] = selected
	}
	if v, exists := input["line"]; exists {
		n, ok := safeInt(v)
		if !ok || n < 1 || len(selected) != 1 {
			return invalidResult("A line needs exactly one path")
		}
		query["line"] = n - 1
	}
	if _, hasName := query["name"]; !hasName && query["line"] == nil && len(selected) == 0 {
		return invalidResult("Name a symbol, a file line or changed paths")
	}
	out, err := runOp(ctx, host, inv, protocol.ToolOperationGraphCode, query)
	if err != nil {
		return faultResult(err)
	}
	page, ok := out.Data.(protocol.CodeIndexGraphGraphPage)
	if !ok || page.View != protocol.CodeIndexGraphViewImpact {
		return invalidResult("Invalid code graph result")
	}
	targets := page.Targets
	truncatedTargets := len(targets) > maxImpactTargets
	if truncatedTargets {
		targets = targets[:maxImpactTargets]
	}
	targetItems := make([]map[string]any, 0, len(targets))
	for _, t := range targets {
		targetItems = append(targetItems, graphDefinition(t))
	}
	fileItems := make([]map[string]any, 0, len(page.Impact))
	for _, f := range page.Impact {
		fileItems = append(fileItems, graphImpacted(f, -1))
	}
	result := graphStatus(page)
	result["targets"] = targetItems
	result["files"] = fileItems
	if truncatedTargets {
		result["partial"], result["truncated"] = true, true
	}
	fitted, err := fitGraphList(result, "files", budget, offset)
	if err != nil {
		return invalidResult(err.Error())
	}
	return engine.ToolResult{Value: fitted}
}

func runCodeMap(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation, raw json.RawMessage) engine.ToolResult {
	input, err := decodeMap(raw, "code map")
	if err != nil {
		return invalidResult(err.Error())
	}
	limit, err := boundInt(input, "limit", defaultGraphItems, maxToolGraphItems, 1)
	if err != nil {
		return invalidResult(err.Error())
	}
	offset, err := boundInt(input, "offset", 0, 10000, 0)
	if err != nil {
		return invalidResult(err.Error())
	}
	budget, err := boundInt(input, "max_bytes", maxCodeBytes, maxCodeBytes, 1024)
	if err != nil {
		return invalidResult(err.Error())
	}
	focus, err := parseGraphPaths(input["paths"])
	if err != nil {
		return invalidResult(err.Error())
	}
	query := map[string]any{"view": "map", "limit": limit, "offset": offset}
	if len(focus) > 0 {
		query["paths"] = focus
	}
	out, err := runOp(ctx, host, inv, protocol.ToolOperationGraphCode, query)
	if err != nil {
		return faultResult(err)
	}
	page, ok := out.Data.(protocol.CodeIndexGraphGraphPage)
	if !ok || page.View != protocol.CodeIndexGraphViewMap {
		return invalidResult("Invalid code graph result")
	}
	files := make([]map[string]any, 0, len(page.Map))
	for _, entry := range page.Map {
		syms := make([]map[string]any, 0, len(entry.Symbols))
		for _, s := range entry.Symbols {
			syms = append(syms, graphRanked(s))
		}
		files = append(files, map[string]any{"path": entry.Path, "score": entry.Score, "symbols": syms})
	}
	result := graphStatus(page)
	result["files"] = files
	fitted, err := fitGraphList(result, "files", budget, offset)
	if err != nil {
		return invalidResult(err.Error())
	}
	return engine.ToolResult{Value: fitted}
}

var pipelineInputs = map[string][]string{
	"code.search":      {"name", "query", "path", "language", "limit"},
	"code.outline":     {"path"},
	"code.graph":       {"view", "name", "paths", "line", "depth", "offset", "limit"},
	"files.find":       {"globs", "path", "offset", "limit"},
	"files.search":     {"query", "regex", "case_sensitive", "globs", "path", "mode", "context", "type", "offset", "limit", "multiline"},
	"files.read_lines": {"path", "offset", "limit", "expected_revision"},
}

func resolvePointer(data any, pointer string) (any, error) {
	if pointer == "" {
		return data, nil
	}
	if !strings.HasPrefix(pointer, "/") {
		return nil, errors.New("Invalid result pointer")
	}
	parts := strings.Split(pointer[1:], "/")
	current := data
	for _, part := range parts {
		for i := 0; i < len(part); i++ {
			if part[i] == '~' && (i+1 >= len(part) || (part[i+1] != '0' && part[i+1] != '1')) {
				return nil, errors.New("Invalid result pointer")
			}
		}
		key := strings.ReplaceAll(strings.ReplaceAll(part, "~1", "/"), "~0", "~")
		switch node := current.(type) {
		case map[string]any:
			val, exists := node[key]
			if !exists {
				return nil, fmt.Errorf("No result at %s", pointer)
			}
			current = val
		case []any:
			if key == "" || (len(key) > 1 && key[0] == '0') {
				return nil, fmt.Errorf("No result at %s", pointer)
			}
			idx, err := strconv.Atoi(key)
			if err != nil || idx < 0 || idx >= len(node) {
				return nil, fmt.Errorf("No result at %s", pointer)
			}
			current = node[idx]
		default:
			return nil, fmt.Errorf("No result at %s", pointer)
		}
	}
	return current, nil
}

func runCodePipeline(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation, hasIndex bool, raw json.RawMessage) engine.ToolResult {
	input, err := decodeMap(raw, "code pipeline")
	if err != nil {
		return invalidResult(err.Error())
	}
	rawSteps, ok := input["steps"].([]any)
	if !ok || len(rawSteps) < 2 || len(rawSteps) > 8 {
		return invalidResult("Expected two to eight dependent reads")
	}
	selectPtr := ""
	if v, exists := input["select"]; exists {
		s, ok := v.(string)
		if !ok {
			return invalidResult("Invalid result pointer")
		}
		selectPtr = s
	}

	var prevData any
	var partialSteps []int
	for stepIdx, rawStep := range rawSteps {
		stepMap, ok := rawStep.(map[string]any)
		if !ok {
			return invalidResult("Unsupported read operation or argument")
		}
		opName, _ := stepMap["operation"].(string)
		if !hasIndex && strings.HasPrefix(opName, "code.") {
			return invalidResult("Unsupported read operation or argument")
		}
		allowedKeys, exists := pipelineInputs[opName]
		if !exists {
			return invalidResult("Unsupported read operation or argument")
		}
		args := map[string]any{}
		if v, ok := stepMap["arguments"]; ok && v != nil {
			m, ok := v.(map[string]any)
			if !ok {
				return invalidResult("Unsupported read operation or argument")
			}
			args = m
		}
		bindings := map[string]string{}
		if v, ok := stepMap["from_previous"]; ok && v != nil {
			m, ok := v.(map[string]any)
			if !ok {
				return invalidResult("Unsupported read operation or argument")
			}
			for k, pv := range m {
				ps, ok := pv.(string)
				if !ok {
					return invalidResult("Invalid result pointer")
				}
				bindings[k] = ps
			}
		}
		for k := range args {
			if !slices.Contains(allowedKeys, k) {
				return invalidResult("Unsupported read operation or argument")
			}
		}
		for k := range bindings {
			if !slices.Contains(allowedKeys, k) {
				return invalidResult("Unsupported read operation or argument")
			}
		}
		if (stepIdx == 0) != (len(bindings) == 0) {
			return invalidResult("Only dependent reads belong in a pipeline; send independent reads together as ordinary tool calls")
		}
		params := make(map[string]any, len(args)+len(bindings))
		for k, v := range args {
			params[k] = v
		}
		for k, ptr := range bindings {
			if _, dup := args[k]; dup {
				return invalidResult("Duplicate argument " + k)
			}
			resolved, err := resolvePointer(prevData, ptr)
			if err != nil {
				return invalidResult(err.Error())
			}
			params[k] = resolved
		}
		if slices.Contains(allowedKeys, "limit") {
			if _, hasLimit := params["limit"]; !hasLimit {
				params["limit"] = 8
			}
			lim, ok := safeInt(params["limit"])
			if !ok || lim < 1 || lim > 64 {
				return invalidResult("Read limit must be between 1 and 64")
			}
		}
		out, err := runOp(ctx, host, inv, protocol.ToolOperation(opName), params)
		if err != nil {
			return faultResult(err)
		}
		encoded, err := json.Marshal(out.Data)
		if err != nil {
			return faultResult(err)
		}
		var decoded any
		dec := json.NewDecoder(strings.NewReader(string(encoded)))
		dec.UseNumber()
		if err := dec.Decode(&decoded); err != nil {
			return faultResult(err)
		}
		if obj, ok := decoded.(map[string]any); ok {
			p, _ := obj["partial"].(bool)
			tr, _ := obj["truncated"].(bool)
			if p || tr || obj["next_offset"] != nil {
				partialSteps = append(partialSteps, stepIdx+1)
			}
		}
		prevData = decoded
	}

	selected, err := resolvePointer(prevData, selectPtr)
	if err != nil {
		return invalidResult(err.Error())
	}
	result := map[string]any{"value": selected}
	if len(partialSteps) > 0 {
		result["partial_steps"] = partialSteps
	}
	if jsonSize(result) > maxCodeBytes {
		return invalidResult("Selected output exceeds 16 KiB; narrow the read or select a smaller field")
	}
	return engine.ToolResult{Value: result}
}
