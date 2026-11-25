package plugins

import (
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const legacyCodePipeline = "code_pipeline"

// ProjectScriptTools hides declarations the host cannot serve and projects
// supported subsets only for known historical package ABIs.
func ProjectScriptTools(ref protocol.PluginReference, declarations []protocol.ToolDeclaration, unavailable func(protocol.ToolOperation) bool) ([]protocol.ToolDeclaration, string) {
	var hiddenInstructionAlias string
	if unavailable(protocol.ToolOperationSearchCode) && ref.Name == "code" && IsLegacyNativeToolPackage(ref.Name, ref.Digest) {
		hiddenInstructionAlias = ToolAlias(ref.Name, legacyCodePipeline)
	}

	projected := make([]protocol.ToolDeclaration, 0, len(declarations))
	for _, declaration := range declarations {
		handler := declaration.Handler
		if handler == nil || declaration.Operation != nil && unavailable(*declaration.Operation) || handler.Operation != nil && unavailable(*handler.Operation) {
			continue
		}
		if handler.Flow != nil && slices.ContainsFunc(handler.Flow.Operations, unavailable) {
			if hiddenInstructionAlias == "" || declaration.Name != legacyCodePipeline {
				continue
			}
			var ok bool
			declaration, ok = fileOnlyLegacyPipeline(declaration, unavailable)
			if !ok {
				continue
			}
		}
		projected = append(projected, declaration)
	}
	return projected, hiddenInstructionAlias
}

func fileOnlyLegacyPipeline(declaration protocol.ToolDeclaration, unavailable func(protocol.ToolOperation) bool) (protocol.ToolDeclaration, bool) {
	handler := *declaration.Handler
	flow := *handler.Flow
	flow.Operations = slices.DeleteFunc(slices.Clone(flow.Operations), unavailable)
	if len(flow.Operations) == 0 {
		return declaration, false
	}
	var schema map[string]any
	if json.Unmarshal(handler.Parameters, &schema) != nil {
		return declaration, false
	}
	properties, _ := schema["properties"].(map[string]any)
	steps, _ := properties["steps"].(map[string]any)
	items, _ := steps["items"].(map[string]any)
	stepProperties, _ := items["properties"].(map[string]any)
	operation, ok := stepProperties["operation"].(map[string]any)
	if !ok {
		return declaration, false
	}
	operation["enum"] = flow.Operations
	parameters, err := json.Marshal(schema)
	if err != nil {
		return declaration, false
	}
	handler.Flow, handler.Parameters = &flow, parameters
	declaration.Handler = &handler
	declaration.Description = new("Chain dependent file reads and return only the selected final data (16 KiB). Use from_previous to bind at least one argument per follow-up, such as the first path from files.find to files.read_lines. Use ordinary calls for independent reads.")
	return declaration, true
}
