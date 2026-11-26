package engine

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDelegationRosterAndAdmissionUseInheritedRoleTools(t *testing.T) {
	t.Parallel()
	f, x := delegationFixture(t)
	builtin := func(name string) *[]protocol.PluginConversationTool {
		return &[]protocol.PluginConversationTool{{Kind: "builtin", Data: &protocol.PluginConversationToolBuiltinPayload{Name: name}}}
	}
	legacy := &[]protocol.PluginConversationTool{{Kind: "package", Data: &protocol.PluginConversationToolPackagePayload{Package: "fixture", Name: "read"}}}
	roles := []protocol.RoleProfile{
		{ID: protocol.NewRoleID(), Revision: 1, Key: "missing", Name: "Missing", Tools: builtin("missing_tool")},
		{Key: "coordinator", Name: "Coordinator", Tools: builtin("spawn_agent")},
		{Key: "reader", Name: "Reader", ReadOnly: true, Tools: builtin("mutate_fixture")},
	}
	read := objectTool("read_fixture", true, true, nil)
	read.LegacyPackage, read.LegacyName = "fixture", "read"
	f.r.tools = []Tool{read, objectTool("mutate_fixture", false, false, nil)}
	x.inv.Turn.Roles.Profiles = roles
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	if spawn := findTool(x.tools, "spawn_agent"); spawn == nil || !strings.Contains(spawn.Definition.Description, "No roles from the captured team are available") {
		t.Fatal("the spawn declaration advertised an unusable roster")
	}
	x.inv.Turn.Roles.Profiles = append(roles, protocol.RoleProfile{Key: "legacy", Name: "Legacy reader", ReadOnly: true, Tools: legacy})
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	roster := f.r.delegationRoster(x, store.AgentState{})
	if !strings.Contains(roster, "legacy (Legacy reader) [read only]") ||
		!strings.Contains(roster, "Unavailable roles (missing required tools): missing (missing: missing_tool), coordinator (missing: spawn_agent), reader (missing: mutate_fixture)") {
		t.Fatalf("inaccurate delegation roster: %s", roster)
	}
	for _, role := range roles {
		args, _ := json.Marshal(map[string]string{"role": role.Key, "task": "Check the fixture"})
		result := f.r.spawnChild(x, canonical.ToolRequest{ID: role.Key, Name: "spawn_agent", Arguments: args}, args)
		if result.Failure == nil || !strings.Contains(result.Failure.Message, "required tools are unavailable") {
			t.Fatalf("role %q was not refused for its tool requirements: %+v", role.Key, result)
		}
	}
	x.inv.Message.References = []protocol.ConversationReference{{Target: protocol.ConversationReferenceTarget{
		Kind: "agent", Data: &protocol.RoleReference{ID: roles[0].ID, Revision: roles[0].Revision},
	}}}
	selected, err := f.r.selectedAgent(x, f.state())
	var fault *protocol.Fault
	if selected || !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.Contains(fault.Message, "required tools are unavailable: missing_tool") {
		t.Fatalf("selected role was not refused for its tool requirements: selected=%t, err=%v", selected, err)
	}
	if turns := f.childTurns(); len(turns) != 0 {
		t.Fatalf("an unavailable role was admitted: %v", turns)
	}

	// A real captured package identity still takes priority over a native
	// migration alias, even if its permissions make the role unavailable.
	shadow := packageTool("fixture", "read", "", false, nil)
	shadow.ReadOnly = false
	f.r.tools = append(f.r.tools, shadow)
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	if got := f.r.delegationRoster(x, store.AgentState{}); strings.Contains(got, "legacy (Legacy reader)") || !strings.Contains(got, "legacy (missing: fixture/read)") {
		t.Fatalf("the legacy selector bypassed the package's captured authority: %s", got)
	}
}
