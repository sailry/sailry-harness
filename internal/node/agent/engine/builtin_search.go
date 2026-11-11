package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"sort"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
)

func builtinMatches(tools []Tool, query string, limit int) []Tool {
	query = strings.TrimSpace(query)
	if names, ok := strings.CutPrefix(query, "select:"); ok {
		selected := []Tool{}
		for _, name := range strings.Split(names, ",") {
			if entry := findTool(tools, strings.TrimSpace(name)); entry != nil && entry.Deferred && !slices.ContainsFunc(selected, func(t Tool) bool { return t.Definition.Name == entry.Definition.Name }) {
				selected = append(selected, *entry)
				if len(selected) == limit {
					break
				}
			}
		}
		return selected
	}
	query = strings.ToLower(query)
	terms := map[string]bool{}
	stop := strings.Fields("and are can could for from get have how into need please that the their them these this those tool tools use using want what when where which with would you")
	for _, term := range strings.FieldsFunc(query, func(c rune) bool { return !unicode.IsLetter(c) && !unicode.IsNumber(c) }) {
		if len([]rune(term)) >= 3 && !slices.Contains(stop, term) {
			terms[term] = true
		}
	}
	type match struct {
		exact bool
		score int
		tool  Tool
	}
	matched := []match{}
	for _, tool := range tools {
		if !tool.Deferred {
			continue
		}
		name, description := strings.ToLower(tool.Definition.Name), strings.ToLower(tool.Definition.Description)
		score := 0
		for term := range terms {
			if strings.Contains(name, term) {
				score += 4
			}
			if strings.Contains(description, term) {
				score++
			}
		}
		exact := name == query
		if exact || score >= 2 {
			matched = append(matched, match{exact, score, tool})
		}
	}
	sort.Slice(matched, func(i, j int) bool {
		a, b := matched[i], matched[j]
		if a.exact != b.exact {
			return a.exact
		}
		if a.score != b.score {
			return a.score > b.score
		}
		return a.tool.Definition.Name < b.tool.Definition.Name
	})
	selected := []Tool{}
	for _, value := range matched[:min(len(matched), limit)] {
		selected = append(selected, value.tool)
	}
	return selected
}
func (r *Runtime) builtinSearch(x *execution) *Tool {
	if !slices.ContainsFunc(x.tools, func(tool Tool) bool { return tool.Deferred }) {
		return nil
	}
	tool := jsonTool("tool_search", "Find additional tools by name or capability, or load exact names with select:name,name. Matching tools become available on the next model request and stay loaded in this conversation. Search before using a capability absent from the current tool list. Discovery does not execute the tools or change their permissions.", `{"type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string","minLength":1},"limit":{"type":"integer","minimum":1,"maximum":8}}}`, true, true, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
		if ctx.Err() != nil || x.ctx.Err() != nil {
			return invalidTool("tool discovery invocation is no longer active")
		}
		var arguments struct {
			Query string `json:"query"`
			Limit *uint8 `json:"limit"`
		}
		if json.Unmarshal(call.Arguments, &arguments) != nil {
			return invalidTool("invalid tool discovery arguments")
		}
		limit := 5
		if arguments.Limit != nil {
			limit = max(1, min(int(*arguments.Limit), 8))
		}
		// Discovery needs the materialized context, not the history.
		var captured canonical.Context
		var loaded []string
		if err := r.db.Read(ctx, func(db *sql.Conn) error {
			var err error
			captured, loaded, err = store.LoadAgentContext(ctx, db, x.inv.Session.ID)
			return err
		}); err != nil {
			return toolFault(err)
		}
		matched := builtinMatches(x.tools, arguments.Query, limit)
		contexts := func(name string) []string {
			values := []string{}
			for _, instruction := range x.inv.Instructions {
				if slices.Contains(instruction.AnyTools, name) {
					values = append(values, instruction.Text)
				}
			}
			return values
		}
		previous := map[string]bool{}
		for _, entry := range x.tools {
			if !entry.Deferred || slices.Contains(loaded, entry.Definition.Name) {
				for _, text := range contexts(entry.Definition.Name) {
					previous[text] = true
				}
			}
		}
		additions := []string{}
		tools := []map[string]string{}
		for _, entry := range matched {
			name := entry.Definition.Name
			tools = append(tools, map[string]string{"name": name, "description": entry.Definition.Description})
			for _, text := range contexts(name) {
				if !previous[text] && !slices.Contains(additions, text) {
					additions = append(additions, text)
				}
			}
			if !slices.Contains(loaded, name) {
				loaded = append(loaded, name)
			}
		}
		slices.Sort(additions)
		addition := strings.Join(additions, "\n\n")
		delta := canonical.Context{LoadedTools: &loaded}
		if addition != "" {
			if captured.Policy == nil {
				return invalidTool("captured policy is unavailable")
			}
			policy := *captured.Policy + "\n\n" + addition
			delta.Policy = &policy
		}
		return ToolResult{Value: map[string]any{"tools": tools, "sailry_context": addition}, Context: &delta}
	})
	return &tool
}
