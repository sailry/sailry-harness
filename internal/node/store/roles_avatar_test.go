package store

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A role's icon may name one of the role avatars; a project's and a team's
// may not, and neither may an avatar the list does not have.
func TestRoleAvatarAppearance(t *testing.T) {
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "review", "Review"
	for icon, valid := range map[string]bool{"avatar-fox": true, "avatar-dino": true, "code": true, "avatar-unicorn": false, "avatar-": false, "fox": false} {
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
		t.Error("a project or team takes a role avatar")
	}
}
