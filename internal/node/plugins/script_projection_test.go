package plugins

import (
	"encoding/json"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectScriptToolsWithoutIndex(t *testing.T) {
	ref := protocol.PluginReference{Name: "code", Digest: legacyNativeToolDigests["code"][0]}
	const file = protocol.ToolOperationReadFileLines
	const indexed = protocol.ToolOperationSearchCode
	needsIndex := func(op protocol.ToolOperation) bool { return op == indexed }
	parameters := json.RawMessage(`{"properties":{"steps":{"items":{"properties":{"operation":{"enum":["files.read_lines","code.search"]}}}}}}`)
	declaration := protocol.ToolDeclaration{Name: "code_pipeline", Handler: &protocol.ToolHandler{
		Parameters: parameters, Flow: &protocol.ToolFlow{Operations: []protocol.ToolOperation{file, indexed}},
	}}
	withOther := []protocol.ToolDeclaration{
		declaration,
		{Name: "index", Handler: &protocol.ToolHandler{Operation: new(indexed)}},
		{Name: "normal", Handler: &protocol.ToolHandler{Operation: new(file)}},
	}
	projected, alias := ProjectScriptTools(ref, withOther, needsIndex)
	if alias != ToolAlias("code", "code_pipeline") || len(projected) != 2 || projected[1].Name != "normal" {
		t.Fatalf("projected tools = %+v, hidden instruction alias = %q", projected, alias)
	}
	narrowed := projected[0]
	if !slices.Equal(narrowed.Handler.Flow.Operations, []protocol.ToolOperation{file}) {
		t.Fatalf("narrowed flow = %+v", narrowed.Handler.Flow)
	}
	const fileOnly = `{"properties":{"steps":{"items":{"properties":{"operation":{"enum":["files.read_lines"]}}}}}}`
	if got := string(narrowed.Handler.Parameters); got != fileOnly {
		t.Fatalf("narrowed schema = %s", got)
	}
	if !slices.Equal(declaration.Handler.Flow.Operations, []protocol.ToolOperation{file, indexed}) || !slices.Equal(declaration.Handler.Parameters, parameters) {
		t.Fatal("adaptation changed the captured declaration")
	}
	for _, foreign := range []protocol.PluginReference{{Name: ref.Name, Digest: "unknown"}, {Name: "other", Digest: ref.Digest}} {
		projected, alias := ProjectScriptTools(foreign, withOther, needsIndex)
		if alias != "" || len(projected) != 1 || projected[0].Name != "normal" {
			t.Fatalf("unrecognized package got the legacy adaptation: %+v, %q", projected, alias)
		}
	}
	projected, alias = ProjectScriptTools(ref, withOther, func(protocol.ToolOperation) bool { return false })
	if alias != "" || len(projected) != 3 || projected[0].Handler != declaration.Handler {
		t.Fatal("available operations did not keep their original declarations")
	}
	declaration.Handler.Flow = &protocol.ToolFlow{Operations: []protocol.ToolOperation{indexed}}
	if tools, _ := ProjectScriptTools(ref, []protocol.ToolDeclaration{declaration}, needsIndex); len(tools) != 0 {
		t.Fatal("flow without file operations was kept")
	}
}
