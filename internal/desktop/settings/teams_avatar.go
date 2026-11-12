package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The avatar picker's grid: every picture avatarCell across, avatarGap
// apart, avatarColumns to a row.
const (
	avatarCell    = 44
	avatarGap     = 8
	avatarColumns = 6
)

// avatarPicker is the role editor's avatar popover: whether it shows, and
// enter, which gives the chosen avatar the focus as it opens.
type avatarPicker struct {
	open, enter bool
}

// avatarName is what an avatar is called: its animal, or "No avatar" for a
// role that shows its mark.
func avatarName(c *ui.Context, avatar string) string {
	if name, ok := strings.CutPrefix(avatar, icons.AvatarPrefix); ok && icons.Avatar(avatar) != nil {
		return i18n.T(c, "avatar_"+name)
	}
	return i18n.T(c, "role_avatar_none")
}

// avatarField is the role editor's Avatar row: a select's trigger showing
// the role as it looks now, with the avatar's name, that opens the
// pictures to choose from. Choosing one sets the draft's avatar and closes
// the popover; the editor's Save stores it with the rest.
func avatarField(c *ui.Context, role *preview.Role, disabled bool) {
	p, th := theme.Of(c), c.Theme()
	label := i18n.T(c, "role_avatar")
	kit.Field(c, "avatar", label, "", func() {
		holder := ui.Row(c.Key("holder"))
		st := ui.Local(holder, "picker", func() avatarPicker { return avatarPicker{} })
		holder.Children(func() {
			// The Select's trigger (kit.SelectTrigger) with the role's
			// picture or mark before the name.
			b := ui.ButtonBase(c.Key("trigger")).Label(label).Disabled(disabled).HasPopup(ui.PopupDialog).Expanded(st.open)
			b.Padding(th.Space(1.5), th.Space(2.5)).Gap(8).MinWidth(180).Radius(p.Radius).
				Border(1, p.Input).Background(th.Surface).AlignItems(ui.Center).Justify(ui.SpaceBetween).Cursor(ui.CursorPointer)
			if b.IsDisabled() {
				b.Opacity(0.5)
			}
			b.Children(func() {
				ui.Row(c).Gap(8).AlignItems(ui.Center).Shrink(1).MinWidth(0).Children(func() {
					kit.RoleAvatar(c, role.ID, role.Avatar, 20)
					ui.Text(c, avatarName(c, role.Avatar)).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
				})
				ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground).Shrink(0)
			}).OnClick(func() { st.open, st.enter = !st.open, true })
			panel := kit.Popover(c, b, &st.open, func() { avatarChoices(c, st, role) })
			if panel != (ui.Element{}) {
				panel.AttachTo(b, ui.AnchorBottomStart, ui.AnchorTopStart).Padding(12)
			}
		})
	})
}

// avatarChoices is the popover's content: No avatar, which gives the role
// its mark again, over the pictures in rows of avatarColumns. The chosen
// picture has a ring in the emphasis color around it, since a fill would
// be under the picture; the one under the pointer has a faint one. Left
// and Right move through the choices, Up and Down by a row, with No avatar
// above the first; Enter or Space chooses.
func avatarChoices(c *ui.Context, st *avatarPicker, role *preview.Role) {
	p := theme.Of(c)
	choose := func(avatar string) { role.Avatar, st.open = avatar, false }
	// The choices in the keys' order: No avatar, then the pictures.
	cells := make([]ui.Element, 0, 1+len(icons.AvatarNames))
	chosen := 0
	body := ui.Column(c.Key("avatars")).Gap(avatarGap).FocusGroup(ui.Horizontal)
	body.Children(func() {
		name := i18n.T(c, "role_avatar_none")
		none := kit.Ghost(c.Key("none"), name, false).FillWidth().Height(32).PaddingX(8).Gap(8).Justify(ui.Start).
			AccessSelected(icons.Avatar(role.Avatar) == nil)
		if icons.Avatar(role.Avatar) == nil {
			none.Background(p.Selected())
		}
		none.Children(func() {
			kit.RoleAvatar(c, role.ID, "", 20)
			ui.Text(c, name).FontSize(14).SingleLine()
		}).OnClick(func() { choose("") })
		cells = append(cells, none)
		width := float32(avatarColumns*avatarCell + (avatarColumns-1)*avatarGap)
		ui.Grid(c.Key("grid")).Columns(avatarColumns).Gap(avatarGap).Width(width).Children(func() {
			for i, animal := range icons.AvatarNames {
				avatar := icons.AvatarPrefix + animal
				selected := role.Avatar == avatar
				if selected {
					chosen = i + 1
				}
				b := ui.ButtonBase(c.Key(animal)).Label(i18n.T(c, "avatar_"+animal)).Size(avatarCell, avatarCell).
					Radius(avatarCell / 4).Cursor(ui.CursorPointer).AccessSelected(selected)
				ring := ui.Color{}
				switch {
				case selected:
					ring = p.Primary
				case b.Hovered() || b.Pressed():
					ring = p.Foreground.Alpha(0.35)
				}
				b.Children(func() {
					kit.Avatar(c, avatar, avatarCell)
					if ring != (ui.Color{}) {
						// 2 DIP wide, 1 DIP clear of the picture, in the
						// gap between two.
						ui.Box(c).Absolute().Top(-3).Left(-3).Size(avatarCell+6, avatarCell+6).Radius(avatarCell/4+3).Border(2, ring)
					}
				}).OnClick(func() { choose(avatar) })
				cells = append(cells, b)
			}
		})
	})
	focused := -1
	for i, cell := range cells {
		if cell.Focused() {
			focused = i
		}
	}
	target := -1
	if st.enter {
		target, st.enter = chosen, false
	}
	if body.Shortcut(0, ui.KeyDown) {
		switch {
		case focused == 0:
			target = 1
		case focused > 0 && focused+avatarColumns < len(cells):
			target = focused + avatarColumns
		}
	}
	if body.Shortcut(0, ui.KeyUp) {
		switch {
		case focused > avatarColumns:
			target = focused - avatarColumns
		case focused > 0:
			target = 0
		}
	}
	if target >= 0 {
		cells[target].Focus()
	}
}
