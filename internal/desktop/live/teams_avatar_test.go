package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A role's avatar is its appearance's icon: read from it, saved to it, and
// accepted by the Node for every picture the Desktop has.
func TestRoleAvatar(t *testing.T) {
	role := protocol.DefaultRoleProfile()
	role.ID, role.Revision, role.Key, role.Name = protocol.RoleID{1}, 3, "review", "Review"
	role.Appearance = &protocol.ProjectsAppearance{Icon: "code", Color: "blue"}
	r := &Runtime{teams: newTeamState()}
	r.teams.roles[profileKey(role.ID.String(), role.Revision)] = role
	draft := roleView(role)
	if draft.Avatar != "" {
		t.Fatalf("a project icon shows as the avatar %q", draft.Avatar)
	}
	draft.Avatar = "avatar-fox"
	saved, err := r.roleDraft(draft)
	if err != nil || saved.Appearance == nil || *saved.Appearance != (protocol.ProjectsAppearance{Icon: "avatar-fox", Color: "blue"}) {
		t.Fatalf("the draft's appearance is %+v, %v", saved.Appearance, err)
	}
	if err := store.ValidateRole(saved); err != nil {
		t.Fatalf("the Node refuses the draft: %v", err)
	}
	// The saved role shows the avatar, and No avatar gives the appearance
	// back to the Node.
	saved.Revision = 4
	r.teams.roles[profileKey(saved.ID.String(), saved.Revision)] = saved
	draft = roleView(saved)
	if draft.Avatar != "avatar-fox" {
		t.Fatalf("the saved role shows the avatar %q", draft.Avatar)
	}
	draft.Avatar = ""
	if cleared, err := r.roleDraft(draft); err != nil || cleared.Appearance != nil {
		t.Fatalf("No avatar keeps %+v, %v", cleared.Appearance, err)
	}
	// An icon this build has no picture of shows the mark and is kept.
	unknown := saved
	unknown.Revision, unknown.Appearance = 5, &protocol.ProjectsAppearance{Icon: "avatar-unicorn", Color: "teal"}
	r.teams.roles[profileKey(unknown.ID.String(), unknown.Revision)] = unknown
	draft = roleView(unknown)
	if kept, err := r.roleDraft(draft); draft.Avatar != "" || err != nil || *kept.Appearance != *unknown.Appearance {
		t.Fatalf("an unknown avatar shows %q and saves %+v, %v", draft.Avatar, kept.Appearance, err)
	}
	// A new role with an avatar has no color yet.
	fresh := roleView(protocol.RoleProfile{Key: "new", Name: "New"})
	fresh.ID, fresh.Avatar = "", "avatar-owl"
	if created, err := r.roleDraft(fresh); err != nil || *created.Appearance != (protocol.ProjectsAppearance{Icon: "avatar-owl", Color: "none"}) || store.ValidateRole(created) != nil {
		t.Fatalf("a new role's appearance is %+v, %v", created.Appearance, err)
	}
	// The Node's list of avatars is the Desktop's.
	for _, name := range icons.AvatarNames {
		saved.Appearance = &protocol.ProjectsAppearance{Icon: icons.AvatarPrefix + name, Color: "none"}
		if icons.Avatar(saved.Appearance.Icon) == nil || store.ValidateRole(saved) != nil {
			t.Errorf("%s: picture %v, Node %v", name, icons.Avatar(saved.Appearance.Icon) != nil, store.ValidateRole(saved))
		}
	}
	// A child shows the avatar of the role it was delegated to.
	child := protocol.DefaultConversationChild()
	if childAvatar([]protocol.RoleProfile{saved}, child) != "" {
		t.Error("a child without a role has an avatar")
	}
	child.Origin.Role = &saved.ID
	if got := childAvatar([]protocol.RoleProfile{saved}, child); got != saved.Appearance.Icon {
		t.Errorf("the child's avatar is %q", got)
	}
}
