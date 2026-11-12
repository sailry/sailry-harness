package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
)

// Avatar draws the picture of a role avatar's identifier (icons.Avatar) in
// a rounded square size across, its corners a quarter of the size. The
// pictures fill their square, so there is no padding or fill behind them.
// It reports false, and draws nothing, for an identifier without a
// picture: the caller draws the role's mark then.
func Avatar(c *ui.Context, avatar string, size float32) (ui.Element, bool) {
	picture := icons.Avatar(avatar)
	if picture == nil {
		return ui.Element{}, false
	}
	return ui.Box(c).Size(size, size).Shrink(0).Radius(size / 4).Clip().Children(func() {
		ui.Image(c, picture).Fill().Fit(ui.Cover)
	}), true
}

// RoleAvatar is how a role shows: its avatar's picture, or the agent mark
// its ID picks, in the ID's color, for a role without an avatar or with
// one this build has no picture of.
func RoleAvatar(c *ui.Context, id, avatar string, size float32) ui.Element {
	if picture, ok := Avatar(c, avatar, size); ok {
		return picture
	}
	return Agent(c, id, AgentColor(c, id), false, size)
}

// RoleIdenticon is a subagent's mark: its role's avatar when the role has
// one, else its Identicon.
func RoleIdenticon(c *ui.Context, id, avatar string, running bool, size float32) ui.Element {
	if picture, ok := Avatar(c, avatar, size); ok {
		return picture
	}
	return Identicon(c, id, running, size)
}
