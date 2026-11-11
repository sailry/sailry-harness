package engine

import (
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChildStandardTools(t *testing.T) {
	bound := func(operation protocol.ToolOperation) *store.ApprovalBinding {
		return &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "fixture"}, Tool: "fixture__tool", Operation: &operation}
	}
	script := &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "fixture"}, Tool: "fixture__tool"}
	named := func(name string) fantasy.FunctionTool { return fantasy.FunctionTool{Name: name} }
	for _, row := range []struct {
		name string
		tool Tool
		keep bool
	}{
		{"reference tool", Tool{Definition: named("read_session")}, true},
		{"other core tool", Tool{Definition: named("ask_user")}, false},
		{"standard operation", Tool{Definition: named("fixture__tool"), Binding: bound(protocol.ToolOperationReadFile)}, true},
		{"code search operation", Tool{Definition: named("fixture__tool"), Binding: bound(protocol.ToolOperationSearchCode)}, true},
		{"artifact revisions operation", Tool{Definition: named("fixture__tool"), Binding: bound(protocol.ToolOperationListArtifactRevisions)}, true},
		{"other operation", Tool{Definition: named("fixture__tool"), Binding: bound(protocol.ToolOperationQueryDatabase)}, false},
		{"read-only hosted script", Tool{Definition: named("fixture__tool"), Binding: script, Hosted: true, ReadOnly: true}, true},
		{"writing hosted script", Tool{Definition: named("fixture__tool"), Binding: script, Hosted: true}, false},
		{"flow of standard operations", Tool{Definition: named("fixture__tool"), Binding: script, Flow: []protocol.ToolOperation{protocol.ToolOperationReadFile, protocol.ToolOperationRunCommand}}, true},
		{"flow with another operation", Tool{Definition: named("fixture__tool"), Binding: script, ReadOnly: true, Flow: []protocol.ToolOperation{protocol.ToolOperationReadFile, protocol.ToolOperationQueryDatabase}}, false},
		{"flow without operations", Tool{Definition: named("fixture__tool"), Binding: script, ReadOnly: true, Flow: []protocol.ToolOperation{}}, false},
		{"unbound package tool", Tool{Definition: named("fixture__server__tool"), Binding: script, ReadOnly: true}, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			if got := roleAllowsTool(nil, row.tool); got != row.keep {
				t.Fatalf("kept=%v", got)
			}
			// A role without a tool list selects the same standard catalog.
			if got := roleAllowsTool(&protocol.RoleProfile{}, row.tool); got != row.keep {
				t.Fatalf("role without a tool list kept=%v", got)
			}
		})
	}
	t.Run("a role tool list stays exact", func(t *testing.T) {
		tools := []protocol.PluginConversationTool{}
		role := &protocol.RoleProfile{Tools: &tools}
		if roleAllowsTool(role, Tool{Definition: named("fixture__tool"), Binding: script, Hosted: true, ReadOnly: true}) {
			t.Fatal("an unlisted hosted tool joined an explicit selection")
		}
	})
}
