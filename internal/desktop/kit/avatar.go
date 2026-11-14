package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Avatars are a picture in a rounded square size across, its corners a
// quarter of the size. A role's own face (icons.Avatar) is on a pale tile
// of its body's color, and a role without one has a white face on a solid
// color, which tells such roles apart by their color alone. A team's
// picture fills its tile.

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

// TeamAvatar is how a team shows: its avatar's Shape Grid, or the one its
// ID picks for a team without one. The pictures fill their tile.
func TeamAvatar(c *ui.Context, id, avatar string, size float32) ui.Element {
	picture := icons.TeamAvatar(avatar)
	if picture == nil {
		picture = icons.DefaultTeamAvatar(id)
	}
	return face(c, picture.Face, ui.Color{}, size)
}

// RoleIdenticon is a subagent's mark: its role's avatar when the role has
// one, else its Identicon.
func RoleIdenticon(c *ui.Context, id, avatar string, running bool, size float32) ui.Element {
	if picture, ok := Avatar(c, avatar, size); ok {
		return picture
	}
	return Identicon(c, id, running, size)
}
