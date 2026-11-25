package engine

import (
	"slices"
	"strings"
	"testing"
	"testing/fstest"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCountedTools(t *testing.T) {
	t.Parallel()
	t.Run("every registered built-in is counted or excluded with a reason", func(t *testing.T) {
		r, x := builtinsFixture(t)
		registered := r.builtins(x)
		// Discovery joins the catalog after the deferral pass.
		if search := r.builtinSearch(x); search != nil {
			registered = append(registered, *search)
		}
		var names []string
		for _, tool := range registered {
			names = append(names, tool.Definition.Name)
		}
		for _, name := range []string{"set_session_title", "ask_user", "list_sessions", "read_session", "search_session", "read_tool_output",
			"spawn_agent", "guide_agent", "wait_agent", "load_skill", "read_skill_resource", "tool_search"} {
			if !slices.Contains(names, name) {
				t.Errorf("the fixture turn does not register %q: %v", name, names)
			}
		}
		for _, name := range names {
			counted, ok := CountedTool(registered, name)
			reason, excluded := telemetry.UncountedBuiltins[name]
			switch {
			case excluded && (ok || reason == ""):
				t.Errorf("excluded built-in %q is counted (%v) or has no reason", name, ok)
			case !excluded && (!ok || counted != (telemetry.Tool{Source: "builtin", Name: name})):
				t.Errorf("registered built-in %q is neither counted under its name nor excluded with a reason: %+v", name, counted)
			}
		}
		for name := range telemetry.UncountedBuiltins {
			if !slices.Contains(names, name) {
				t.Errorf("excluded built-in %q is not a registered tool", name)
			}
		}
		if counted, ok := CountedTool(registered, "write_file"); ok {
			t.Errorf("an unregistered name is counted: %+v", counted)
		}
		nativeBound := []Tool{{Definition: fantasy.FunctionTool{Name: "read_file"}, Binding: &store.ApprovalBinding{Tool: "read_file"}}}
		if counted, ok := CountedTool(nativeBound, "read_file"); !ok || counted != (telemetry.Tool{Source: "builtin", Name: "read_file"}) {
			t.Errorf("native module tool with empty plugin owner was misclassified as %+v (%v)", counted, ok)
		}
	})

	t.Run("installed package names do not establish official identity", func(t *testing.T) {
		bound := func(owner protocol.PluginReference, name string) []Tool {
			return []Tool{{Definition: fantasy.FunctionTool{Name: name}, Binding: &store.ApprovalBinding{Owner: owner, Tool: name}}}
		}
		for _, owner := range []protocol.PluginReference{
			{Name: "files"},
			{Name: "files", Digest: strings.Repeat("0", 64)},
			{Name: "my-private-package", Digest: strings.Repeat("0", 64)},
		} {
			for _, name := range []string{operations.Alias(owner.Name, "read_file"), operations.Alias(owner.Name, "private_tool")} {
				if counted, ok := CountedTool(bound(owner, name), name); !ok || counted != (telemetry.Tool{Source: "third_party"}) {
					t.Errorf("tool %q of %q is counted as %+v (%v)", name, owner.Name, counted, ok)
				}
			}
			const connected = "mcp_private_server_tool_0123456789abcdef01234567"
			if counted, ok := CountedTool(bound(owner, connected), connected); !ok || counted != (telemetry.Tool{Source: "mcp"}) {
				t.Errorf("a connected server's tool is counted as %+v (%v)", counted, ok)
			}
		}
	})
}

// builtinsFixture enables delegation, skills and deferred-tool discovery without
// depending on the set of packages installed or shipped with the application.
func builtinsFixture(t *testing.T) (*Runtime, *execution) {
	t.Helper()
	f, x := delegationFixture(t)
	x.inv.Session.Delegation, x.inv.Turn.Config.Resource = nil, nil
	x.inv.Turn.Roles.Execution = protocol.ConversationExecutionTeam
	resources, err := operations.CaptureResources(x.ctx, f.r.profile, nil)
	if err != nil {
		t.Fatal(err)
	}
	resources.AddNativeSkills([]operations.NativeSkillPackage{{
		Package: "fixture",
		Skills:  []protocol.PluginSkill{{Name: "inspect", Description: "Inspect the fixture", Path: "skills/inspect/SKILL.md"}},
		Tree:    fstest.MapFS{"skills/inspect/SKILL.md": {Data: []byte("Fixture skill instructions")}},
	}})
	x.resources = resources
	x.tools = []Tool{{Definition: fantasy.FunctionTool{Name: "deferred"}, Deferred: true}}
	return f.r, x
}
