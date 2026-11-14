package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Avatars are a face in a rounded square size across, its corners a
// quarter of the size: a role's own face (icons.Avatar) on a pale tile of
// its body's color, or a white face on a solid color, which tells roles
// without one apart by their color alone.

// face draws picture in a tile of fill.
func face(c *ui.Context, picture *ui.SVG, fill ui.Color, size float32) ui.Element {
	return ui.Box(c).Size(size, size).Shrink(0).Radius(size / 4).Clip().Background(fill).Children(func() {
		ui.Image(c, picture).Fill().Fit(ui.Contain)
	})
}

// avatarTint is how strongly a face's body color fills its tile: enough to
// tell the tile from the page, light enough for the face to stand out.
func avatarTint(c *ui.Context) float32 {
	if theme.Of(c).Dark {
		return 0.22
	}
	return 0.3
}

// Avatar draws the picture of a role avatar's identifier. It reports
// false, and draws nothing, for an identifier without a picture: the
// caller draws the role's default then.
func Avatar(c *ui.Context, avatar string, size float32) (ui.Element, bool) {
	picture := icons.Avatar(avatar)
	if picture == nil {
		return ui.Element{}, false
	}
	return face(c, picture.Face, picture.Body.Alpha(avatarTint(c)), size), true
}

// DefaultAvatar is a role without an avatar: the default face on the
// color its ID picks.
func DefaultAvatar(c *ui.Context, id string, size float32) ui.Element {
	return face(c, icons.DefaultFace(), AgentColor(c, id), size)
}

// LeadAvatar is a team's lead: its own face on the primary color.
func LeadAvatar(c *ui.Context, size float32) ui.Element {
	return face(c, icons.LeadFace(), theme.Of(c).Primary, size)
}

// RoleAvatar is how a role shows: its avatar's picture, or the default
// face on its color for a role without an avatar or with one this build
// has no picture of.
func RoleAvatar(c *ui.Context, id, avatar string, size float32) ui.Element {
	if picture, ok := Avatar(c, avatar, size); ok {
		return picture
	}
	return DefaultAvatar(c, id, size)
}

// TeamMember is a member a team's avatar shows: its role's ID and avatar.
type TeamMember struct{ ID, Avatar string }

// TeamAvatar is a team: its first four members' faces in a 2 by 2 grid on
// a pale tile of the team's color, as a group chat shows; a team of one
// shows that member, and an empty team the default face on its color.
func TeamAvatar(c *ui.Context, id string, members []TeamMember, size float32) ui.Element {
	switch len(members) {
	case 0:
		return DefaultAvatar(c, id, size)
	case 1:
		return RoleAvatar(c, members[0].ID, members[0].Avatar, size)
	}
	members = members[:min(4, len(members))]
	pad := max(2, size/16)
	cell := (size - 3*pad) / 2
	return ui.Box(c).Size(size, size).Shrink(0).Radius(size / 4).Clip().Background(AgentColor(c, id).Alpha(avatarTint(c))).Children(func() {
		for i, m := range members {
			x, y := pad+float32(i%2)*(cell+pad), pad+float32(i/2)*(cell+pad)
			switch {
			case len(members) == 2:
				// Two side by side, centered down the tile.
				y = (size - cell) / 2
			case len(members) == 3 && i == 2:
				// The third centers under the first two.
				x = (size - cell) / 2
			}
			ui.Box(c.Key(i)).Absolute().Left(x).Top(y).Children(func() { RoleAvatar(c, m.ID, m.Avatar, cell) })
		}
	})
}

// RoleIdenticon is a subagent's mark: its role's avatar when the role has
// one, else its Identicon.
func RoleIdenticon(c *ui.Context, id, avatar string, running bool, size float32) ui.Element {
	if picture, ok := Avatar(c, avatar, size); ok {
		return picture
	}
	return Identicon(c, id, running, size)
}
