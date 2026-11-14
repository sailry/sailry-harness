package store

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A role's and a team's icon may name a generated avatar by its seed; a
// project's may not, and no icon may name one by anything else.
func TestRoleAvatarAppearance(t *testing.T) {
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "review", "Review"
	for icon, valid := range map[string]bool{"avatar-fox": true, "avatar-k3f9x2a1": true, "avatar-" + strings.Repeat("a", 32): true, "code": true, "avatar-" + strings.Repeat("a", 33): false, "avatar-Fox": false, "avatar-fox-1": false, "avatar-": false, "fox": false} {
		role.Appearance = &protocol.ProjectsAppearance{Icon: icon, Color: "teal"}
		if err := ValidateRole(role); (err == nil) != valid {
			t.Errorf("role icon %q: %v", icon, err)
		}
	}
	role.Appearance = &protocol.ProjectsAppearance{Icon: "avatar-fox", Color: "pink"}
	if ValidateRole(role) == nil {
		t.Error("an avatar passes with an unknown color")
	}
	if validateAppearance(protocol.ProjectsAppearance{Icon: "avatar-fox", Color: "teal"}) == nil {
		t.Error("a project takes a generated avatar")
	}
	// A team takes the same avatars.
	team := protocol.DefaultTeamProfile()
	team.ID, team.Key, team.Name = protocol.NewTeamID(), "ship", "Ship"
	for icon, valid := range map[string]bool{"avatar-k3f9x2a1": true, "code": true, "avatar-Fox": false} {
		team.Appearance = &protocol.ProjectsAppearance{Icon: icon, Color: "none"}
		if err := ValidateTeam(team); (err == nil) != valid {
			t.Errorf("team icon %q: %v", icon, err)
		}
	}
}
