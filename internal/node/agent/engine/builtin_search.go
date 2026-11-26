package engine

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
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
	stop := strings.Fields("am an and are as at be by can could do for from get have how if in into is it me my need no of on or please so that the their them these this those to tool tools up us use using want we what when where which with would you")
	for _, term := range strings.FieldsFunc(query, func(c rune) bool { return !unicode.IsLetter(c) && !unicode.IsNumber(c) }) {
		characters := []rune(term)
		if len(characters) >= 2 && !slices.Contains(stop, term) {
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
		labels := make([]string, len(tool.SearchLabels))
		for index, label := range tool.SearchLabels {
			labels[index] = strings.ToLower(strings.TrimSpace(label))
		}
		score := 0
		for term := range terms {
			if strings.Contains(name, term) {
				score += 4
			}
			if strings.Contains(description, term) {
				score += 2
			}
			if slices.ContainsFunc(labels, func(label string) bool { return strings.Contains(label, term) }) {
				score += 4
			}
		}
		// A localized label may appear inside a sentence without word spaces.
		if slices.ContainsFunc(labels, func(label string) bool { return label != "" && strings.Contains(query, label) }) {
			score += 4
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

// capabilityInstructions adds the policy of the tools every request declares.
// Loaded deferred tools contribute through loadedToolContext instead, so
// discovery never rewrites the system prompt that provider caches key on.
func capabilityInstructions(x *execution, policy string) string {
	var additions []string
	var offered []string
	for _, tool := range x.tools {
		if tool.Deferred {
			continue
		}
		offered = append(offered, tool.Definition.Name)
		if tool.Instruction != "" && !strings.Contains(policy, tool.Instruction) && !slices.Contains(additions, tool.Instruction) {
			additions = append(additions, tool.Instruction)
		}
	}
	for _, instruction := range x.inv.Instructions {
		if instruction.Text == "" || strings.Contains(policy, instruction.Text) || slices.Contains(additions, instruction.Text) {
			continue
		}
		if slices.ContainsFunc(instruction.AnyTools, func(name string) bool {
			return slices.Contains(offered, name)
		}) {
			additions = append(additions, instruction.Text)
		}
	}
	slices.Sort(additions)
	return strings.Join(additions, "\n\n")
}

// loadedToolContext describes loaded deferred tools as owned turn context:
// each declaration, then package instructions selected only by loaded tools.
// Owned content is appended once after the history and restored when
// compaction removes it, so loading leaves the declared tool list and system
// prompt byte-identical.
func loadedToolContext(x *execution, policy string, loaded []string) []string {
	var blocks, selected, instructions []string
	for _, tool := range x.tools {
		if !tool.Deferred || !toolLoaded(loaded, tool, x.names) {
			continue
		}
		selected = append(selected, tool.Definition.Name)
		var schema bytes.Buffer
		if len(tool.SchemaJSON) == 0 || json.Compact(&schema, tool.SchemaJSON) != nil {
			encoded, _ := json.Marshal(tool.Definition.InputSchema)
			schema.Write(encoded)
		}
		blocks = append(blocks, fmt.Sprintf("<loaded-tool name=%q>\n%s\nInput schema: %s\n</loaded-tool>", x.names.name(tool.Definition.Name), tool.Definition.Description, schema.Bytes()))
		if tool.Instruction != "" && !strings.Contains(policy, tool.Instruction) && !slices.Contains(instructions, tool.Instruction) {
			instructions = append(instructions, tool.Instruction)
		}
	}
	for _, instruction := range x.inv.Instructions {
		if instruction.Text == "" || strings.Contains(policy, instruction.Text) || slices.Contains(instructions, instruction.Text) {
			continue
		}
		if slices.ContainsFunc(instruction.AnyTools, func(name string) bool { return slices.Contains(selected, name) }) {
			instructions = append(instructions, instruction.Text)
		}
	}
	slices.Sort(instructions)
	for _, text := range instructions {
		blocks = append(blocks, "<loaded-tool-instructions>\n"+text+"\n</loaded-tool-instructions>")
	}
	return blocks
}

// callTool is the stable entry for deferred tools. The model response
// boundary replaces a call that names a captured tool with that tool's own
// call before approval, execution and storage; this executor answers only
// calls that name no captured tool.
func callTool() Tool {
	return jsonTool(callToolName, "Call a tool loaded with tool_search. Pass its exact name and, as arguments, an object that follows the input schema in its loaded-tool context. The call runs as that tool, under its own approval and permissions.", `{"type":"object","additionalProperties":false,"required":["name","arguments"],"properties":{"name":{"type":"string","minLength":1},"arguments":{"type":"object"}}}`, true, true, func(_ context.Context, call canonical.ToolRequest) ToolResult {
		var arguments struct {
			Name string `json:"name"`
		}
		_ = json.Unmarshal(call.Arguments, &arguments)
		return invalidTool(fmt.Sprintf("%q is not an available tool; use tool_search to find and load it", arguments.Name))
	})
}

func (r *Runtime) builtinSearch(x *execution) *Tool {
	if !slices.ContainsFunc(x.tools, func(tool Tool) bool { return tool.Deferred }) {
		return nil
	}
	tool := jsonTool("tool_search", "Find additional tools by name or capability, or load exact names with select:name,name. Matching tools are loaded: their input schemas appear as loaded-tool context before the next model request, and call_tool runs them. Context compaction can unload tools; search again for a tool whose loaded-tool context is absent. Discovery does not execute the tools or change their permissions.", `{"type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string","minLength":1},"limit":{"type":"integer","minimum":1,"maximum":8}}}`, true, true, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
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
		var loaded []string
		if err := r.db.Read(ctx, func(db *sql.Conn) error {
			var err error
			_, loaded, err = store.LoadAgentContext(ctx, db, x.inv.Session.ID)
			return err
		}); err != nil {
			return toolFault(err)
		}
		matched := builtinMatches(x.tools, arguments.Query, limit)
		tools := []map[string]string{}
		for _, entry := range matched {
			name := entry.Definition.Name
			tools = append(tools, map[string]string{"name": name})
			if !slices.Contains(loaded, name) {
				loaded = append(loaded, name)
			}
		}
		return ToolResult{Value: map[string]any{"tools": tools}, Context: &canonical.Context{LoadedTools: &loaded}}
	})
	return &tool
}
