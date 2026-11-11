package engine

import (
	"encoding/hex"
	"encoding/json"
	"slices"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"lukechampine.com/blake3"
)

// Provider tool names allow at most 64 characters from [A-Za-z0-9_-].
const (
	maxToolName   = 64
	toolHashChars = 8
)

// exposure maps the internal aliases of package and MCP tools to the names a
// model sees: a package tool's declared name and mcp__{server}__{tool}. The
// aliases stay canonical in records, approvals, rules, hooks and telemetry;
// only a model request and its response cross this boundary.
type exposure struct {
	forward map[string]string
	reverse map[string]string
}

// MCPToolName is the name an MCP tool is offered under when it is unique.
func MCPToolName(server, tool string) string {
	return "mcp__" + toolLabel(server) + "__" + toolLabel(tool)
}

func toolLabel(label string) string {
	return strings.Map(func(character rune) rune {
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '_' || character == '-' {
			return character
		}
		return '_'
	}, label)
}

func providerToolName(name string) bool {
	return name != "" && len(name) <= maxToolName && toolLabel(name) == name
}

func pluginAlias(name string) bool {
	digest, tool, found := strings.Cut(strings.TrimPrefix(name, "plugin_"), "_")
	return strings.HasPrefix(name, "plugin_") && found && len(digest) == 16 && tool != "" && !strings.ContainsFunc(digest, func(c rune) bool { return !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') })
}

func mcpAlias(name string) bool {
	rest, found := strings.CutPrefix(name, "mcp_")
	index := strings.LastIndexByte(rest, '_')
	if !found || index <= 0 {
		return false
	}
	label, digest := rest[:index], rest[index+1:]
	return toolLabel(label) == label && len(digest) == 24 && !strings.ContainsFunc(digest, func(c rune) bool { return !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') })
}

// captureExposure reserves every current and historical identity, including
// deferred tools: a declared name is used only when no other tool, alias or
// earlier call owns it. Equal or overlong MCP names take a short hash.
func captureExposure(tools []Tool, historical []string) *exposure {
	registered := map[string]bool{}
	for _, tool := range tools {
		registered[tool.Definition.Name] = true
	}
	for _, name := range historical {
		registered[name] = true
	}
	type entry struct{ alias, name string }
	var declared, servers []entry
	counts := map[string]int{}
	for _, tool := range tools {
		alias := tool.Definition.Name
		if tool.Binding == nil || tool.Binding.Owner.Name == "" {
			continue
		}
		if tool.Exposed != "" {
			servers = append(servers, entry{alias, tool.Exposed})
			counts[tool.Exposed]++
			continue
		}
		if !pluginAlias(alias) {
			continue
		}
		name := alias[len("plugin_")+17:]
		if operations.Alias(tool.Binding.Owner.Name, name) == alias && providerToolName(name) {
			declared = append(declared, entry{alias, name})
			counts[name]++
		}
	}
	for _, server := range servers {
		name := server.name
		if counts[name] != 1 || len(name) > maxToolName || registered[name] {
			digest := blake3.Sum256([]byte(server.alias))
			name = name[:min(len(name), maxToolName-toolHashChars-1)] + "_" + hex.EncodeToString(digest[:])[:toolHashChars]
		}
		declared = append(declared, entry{server.alias, name})
	}
	counts = map[string]int{}
	for _, value := range declared {
		counts[value.name]++
	}
	result := &exposure{forward: map[string]string{}, reverse: map[string]string{}}
	for _, value := range declared {
		if counts[value.name] == 1 && !registered[value.name] && !pluginAlias(value.name) && !mcpAlias(value.name) {
			result.forward[value.alias] = value.name
			result.reverse[value.name] = value.alias
		}
	}
	return result
}

// exposure captures the names once per turn, on the first step that needs
// them, so history, declarations and discovery agree for the whole turn.
func (r *Runtime) exposure(x *execution, state store.AgentState) *exposure {
	if x.names == nil {
		var historical []string
		for index := range state.Records {
			if message := state.Records[index].Message(); message != nil {
				for _, block := range message.Content {
					if request, ok := block.Data.(*canonical.ToolRequest); ok {
						historical = append(historical, request.Name)
					}
				}
			}
		}
		x.names = captureExposure(x.tools, historical)
	}
	return x.names
}

func (e *exposure) name(alias string) string {
	if e != nil {
		if name, ok := e.forward[alias]; ok {
			return name
		}
	}
	return alias
}

func (e *exposure) alias(name string) string {
	if e != nil {
		if alias, ok := e.reverse[name]; ok {
			return alias
		}
	}
	return name
}

// request renames the tools, their schemas and the tool calls of the history
// for one model request. The stored records are shared and stay untouched.
func (e *exposure) request(records []canonical.Record, tools []fantasy.FunctionTool, schemas map[string]json.RawMessage) ([]canonical.Record, []fantasy.FunctionTool, map[string]json.RawMessage) {
	if e == nil || len(e.forward) == 0 {
		return records, tools, schemas
	}
	tools = slices.Clone(tools)
	for index := range tools {
		tools[index].Name = e.name(tools[index].Name)
	}
	renamed := make(map[string]json.RawMessage, len(schemas))
	for name, schema := range schemas {
		renamed[e.name(name)] = schema
	}
	records = slices.Clone(records)
	searches := map[string]bool{}
	for index := range records {
		message := records[index].Message()
		if message == nil {
			continue
		}
		var content []canonical.Block
		replace := func(position int, data any) {
			if content == nil {
				content = slices.Clone(message.Content)
			}
			content[position].Data = data
		}
		for position, block := range message.Content {
			switch value := block.Data.(type) {
			case *canonical.ToolRequest:
				if value.Error != nil {
					continue
				}
				if value.Name == "tool_search" {
					searches[value.ID] = true
					if arguments, changed := e.query(value.Arguments, e.forward); changed {
						call := *value
						call.Arguments = arguments
						replace(position, &call)
					}
				} else if name := e.name(value.Name); name != value.Name {
					call := *value
					call.Name = name
					replace(position, &call)
				}
			case *canonical.ToolResponse:
				if searches[value.ID] {
					if result, changed := e.discovered(value.Result.Value); changed {
						response := *value
						response.Result = canonical.Payload{Value: result}
						replace(position, &response)
					}
				} else if name := e.name(value.Name); name != value.Name {
					response := *value
					response.Name = name
					replace(position, &response)
				}
			}
		}
		if content != nil {
			copied := *message
			copied.Content = content
			records[index].Data = &copied
		}
	}
	return records, tools, renamed
}

// restore gives the calls of a model response their canonical aliases.
func (e *exposure) restore(blocks []canonical.Block) {
	if e == nil || len(e.reverse) == 0 {
		return
	}
	for _, block := range blocks {
		call, ok := block.Data.(*canonical.ToolRequest)
		if !ok || call.Error != nil {
			continue
		}
		call.Name = e.alias(call.Name)
		if call.Name == "tool_search" {
			if arguments, changed := e.query(call.Arguments, e.reverse); changed {
				call.Arguments = arguments
			}
		}
	}
}

// query maps the exact names of a discovery query; keyword text is untouched.
func (e *exposure) query(arguments json.RawMessage, names map[string]string) (json.RawMessage, bool) {
	var input map[string]json.RawMessage
	var text string
	if json.Unmarshal(arguments, &input) != nil || json.Unmarshal(input["query"], &text) != nil {
		return nil, false
	}
	mapped := text
	if selected, ok := strings.CutPrefix(strings.TrimSpace(text), "select:"); ok {
		parts := strings.Split(selected, ",")
		for index, part := range parts {
			parts[index] = strings.TrimSpace(part)
			if name, exists := names[parts[index]]; exists {
				parts[index] = name
			}
		}
		mapped = "select:" + strings.Join(parts, ",")
	} else if name, exists := names[strings.TrimSpace(text)]; exists {
		mapped = name
	}
	if mapped == text {
		return nil, false
	}
	input["query"], _ = json.Marshal(mapped)
	body, err := json.Marshal(input)
	return body, err == nil
}

// discovered renames the name fields of a discovery result. Descriptions and
// package context stay as written, including text that resembles an alias.
func (e *exposure) discovered(result json.RawMessage) (json.RawMessage, bool) {
	var value map[string]any
	if json.Unmarshal(result, &value) != nil {
		return nil, false
	}
	tools, ok := value["tools"].([]any)
	if !ok {
		return nil, false
	}
	changed := false
	for _, tool := range tools {
		entry, ok := tool.(map[string]any)
		name, named := entry["name"].(string)
		if !ok || !named {
			return nil, false
		}
		if exposed := e.name(name); exposed != name {
			entry["name"], changed = exposed, true
		}
	}
	if !changed {
		return nil, false
	}
	body, err := json.Marshal(value)
	return body, err == nil
}
