package engine

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSpawnGuidancePreservesSelectedTeamInstructions(t *testing.T) {
	t.Parallel()
	f, x := delegationFixture(t)
	x.inv.Turn.Config.Mode = protocol.WorkModeFlagship
	x.inv.Turn.Roles.Team = &protocol.TeamReference{}
	x.inv.Turn.Roles.Instructions = ""
	x.inv.Turn.Roles.Profiles = nil
	const teamPermission = "team instructions allow delegation without a role"
	const rolelessFallback = "only if no team is selected or the selected team's instructions allow delegation without a role"
	for _, test := range []struct {
		name, description string
		profiles          []protocol.RoleProfile
	}{
		{name: "no available roles", description: teamPermission},
		{name: "fitting role", description: rolelessFallback, profiles: []protocol.RoleProfile{{Key: "reviewer", Name: "Reviewer"}}},
	} {
		t.Run(test.name, func(t *testing.T) {
			x.inv.Turn.Roles.Profiles = test.profiles
			if err := f.r.captureTools(x); err != nil {
				t.Fatal(err)
			}
			spawn := findTool(x.tools, "spawn_agent")
			if spawn == nil || !strings.Contains(spawn.Definition.Description, test.description) {
				t.Fatal("spawn description did not preserve the selected team's permission")
			}
			if roster := f.r.delegationRoster(x, store.AgentState{}); !strings.Contains(roster, teamPermission) {
				t.Fatalf("roster did not preserve the selected team's permission: %s", roster)
			}
			var schema struct {
				Properties map[string]struct {
					Description string `json:"description"`
				} `json:"properties"`
			}
			if err := json.Unmarshal(spawn.SchemaJSON, &schema); err != nil {
				t.Fatal(err)
			}
			if got := schema.Properties["role"].Description; !strings.Contains(got, rolelessFallback) {
				t.Fatalf("role schema did not preserve the selected team's permission: %s", got)
			}
		})
	}
}
