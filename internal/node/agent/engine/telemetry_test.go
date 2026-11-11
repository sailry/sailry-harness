package engine

import (
	"slices"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestCountedTools derives the counted built-ins from the tools a turn
// registers and names a package's tool only for a package the application
// ships.
func TestCountedTools(t *testing.T) {
	catalog, err := operations.Bundled()
	if err != nil || len(catalog.Tools) == 0 {
		t.Fatal(err)
	}

	t.Run("every registered built-in is counted or excluded with a reason", func(t *testing.T) {
		r, x := builtinsFixture(t, catalog)
		registered := r.builtins(x)
		var names []string
		for _, tool := range registered {
			names = append(names, tool.Definition.Name)
		}
		// The turn enables every conditional group, so none escapes the check.
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
		// A name the turn did not register is a model's invention.
		if counted, ok := CountedTool(registered, "write_file"); ok {
			t.Errorf("an unregistered name is counted: %+v", counted)
		}
	})

	t.Run("only a shipped package's tool is named", func(t *testing.T) {
		bound := func(owner protocol.PluginReference, name string) []Tool {
			return []Tool{{Definition: fantasy.FunctionTool{Name: name}, Binding: &store.ApprovalBinding{Owner: owner, Tool: name}}}
		}
		// Every tool of every shipped package has a reportable name.
		for _, declaration := range catalog.Tools {
			want := telemetry.Tool{Source: "official", Name: declaration.Owner.Name + "." + declaration.Exposed}
			if counted, ok := CountedTool(bound(declaration.Owner, declaration.Name), declaration.Name); !ok || counted != want {
				t.Errorf("shipped tool %s of %s is counted as %+v (%v), want %+v", declaration.Exposed, declaration.Owner.Name, counted, ok, want)
			}
		}
		shipped := catalog.Tools[0]
		unnamed := telemetry.Tool{Source: "third_party"}
		// A package a user installed under a shipped package's name has the
		// same alias for the same tool, and another digest.
		planted := protocol.PluginReference{Name: shipped.Owner.Name, Digest: "0000000000000000000000000000000000000000000000000000000000000000"}
		if counted, ok := CountedTool(bound(planted, shipped.Name), shipped.Name); !ok || counted != unnamed {
			t.Errorf("a user-installed package named %q is counted as %+v (%v)", shipped.Owner.Name, counted, ok)
		}
		if counted, ok := CountedTool(bound(protocol.PluginReference{Name: shipped.Owner.Name}, shipped.Name), shipped.Name); !ok || counted != unnamed {
			t.Errorf("a package without the shipped digest is counted as %+v (%v)", counted, ok)
		}
		// A shipped package cannot lend its name to a tool it does not declare,
		// nor a private package gain one.
		for _, name := range []string{operations.Alias(shipped.Owner.Name, "private_tool"), operations.Alias("my-private-package", shipped.Exposed)} {
			for _, owner := range []protocol.PluginReference{shipped.Owner, {Name: "my-private-package", Digest: shipped.Owner.Digest}} {
				if counted, ok := CountedTool(bound(owner, name), name); !ok || counted != unnamed {
					t.Errorf("tool %q of %q is counted as %+v (%v)", name, owner.Name, counted, ok)
				}
			}
		}
		// A connected server's tool is never named, whoever declares it.
		for _, owner := range []protocol.PluginReference{shipped.Owner, planted} {
			if counted, ok := CountedTool(bound(owner, "mcp_private_server_tool_0123456789abcdef01234567"), "mcp_private_server_tool_0123456789abcdef01234567"); !ok || counted != (telemetry.Tool{Source: "mcp"}) {
				t.Errorf("a connected server's tool is counted as %+v (%v)", counted, ok)
			}
		}
	})
}

// builtinsFixture is a turn in which every conditional group of built-in
// tools applies: delegation, skills and deferred-tool discovery.
func builtinsFixture(t *testing.T, catalog *operations.Catalog) (*Runtime, *execution) {
	t.Helper()
	f, x := delegationFixture(t)
	x.inv.Session.Delegation, x.inv.Turn.Config.Resource = nil, nil
	x.inv.Turn.Roles.Execution = protocol.ConversationExecutionTeam
	resources, err := operations.CaptureResources(x.ctx, f.r.profile, catalog.References, nil)
	if err != nil {
		t.Fatal(err)
	}
	x.resources = resources
	x.tools = []Tool{{Definition: fantasy.FunctionTool{Name: "deferred"}, Deferred: true}}
	return f.r, x
}
