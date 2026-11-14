package settings

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The avatar picker's grid: every picture avatarCell across, avatarGap
// apart, avatarColumns to a row, avatarRows rows.
const (
	avatarCell    = 44
	avatarGap     = 8
	avatarColumns = 6
	avatarRows    = 4
)

// avatarPicker is the role editor's avatar popover: whether it shows,
// enter, which gives the chosen avatar the focus as it opens, and the
// avatars it offers.
type avatarPicker struct {
	open, enter bool
	choices     []string
}

// deal fills the grid with new random avatars after the chosen one, which
// stays first so the grid always shows it.
func (st *avatarPicker) deal(chosen string) {
	st.choices = st.choices[:0]
	if icons.Avatar(chosen) != nil {
		st.choices = append(st.choices, chosen)
	}
	for len(st.choices) < avatarColumns*avatarRows {
		if avatar := icons.RandomAvatar(); !slices.Contains(st.choices, avatar) {
			st.choices = append(st.choices, avatar)
		}
	}
}

// avatarName is what an avatar is called: "Avatar" and its seed, or "No
// avatar" for a role that shows its mark.
func avatarName(c *ui.Context, avatar string) string {
	if seed, ok := protocol.RoleAvatarSeed(avatar); ok {
		return i18n.T(c, "role_avatar_seed", "seed", seed)
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
			}).OnClick(func() {
				if st.open, st.enter = !st.open, true; st.open {
					st.deal(role.Avatar)
				}
			})
			panel := kit.Popover(c, b, &st.open, func() { avatarChoices(c, st, role) })
			if panel != (ui.Element{}) {
				panel.AttachTo(b, ui.AnchorBottomStart, ui.AnchorTopStart).Padding(12)
			}
		})
	})
}

// avatarChoices is the popover's content: No avatar, which gives the role
// its mark again, and Shuffle, which deals new random pictures, over the
// pictures in rows of avatarColumns. The chosen
// picture has a ring in the emphasis color around it, since a fill would
// be under the picture; the one under the pointer has a faint one. Left
// and Right move through the choices, Up and Down by a row, with No avatar
// above the first; Enter or Space chooses.
func avatarChoices(c *ui.Context, st *avatarPicker, role *preview.Role) {
	p := theme.Of(c)
	choose := func(avatar string) { role.Avatar, st.open = avatar, false }
	// The choices in the keys' order: No avatar, then the pictures.
	cells := make([]ui.Element, 0, 1+len(st.choices))
	chosen := 0
	body := ui.Column(c.Key("avatars")).Gap(avatarGap).FocusGroup(ui.Horizontal)
	body.Children(func() {
		ui.Row(c.Key("head")).Gap(avatarGap).AlignItems(ui.Center).Children(func() {
			name := i18n.T(c, "role_avatar_none")
			none := kit.Ghost(c.Key("none"), name, false).Grow(1).Height(32).PaddingX(8).Gap(8).Justify(ui.Start).
				AccessSelected(icons.Avatar(role.Avatar) == nil)
			if icons.Avatar(role.Avatar) == nil {
				none.Background(p.Selected())
			}
			none.Children(func() {
				kit.RoleAvatar(c, role.ID, "", 20)
				ui.Text(c, name).FontSize(14).SingleLine()
			}).OnClick(func() { choose("") })
			cells = append(cells, none)
			kit.IconButton(c.Key("shuffle"), icons.Shuffle, i18n.T(c, "role_avatar_shuffle"), 32, false).
				OnClick(func() { st.deal(role.Avatar) })
		})
		width := float32(avatarColumns*avatarCell + (avatarColumns-1)*avatarGap)
		ui.Grid(c.Key("grid")).Columns(avatarColumns).Gap(avatarGap).Width(width).Children(func() {
			for i, avatar := range st.choices {
				selected := role.Avatar == avatar
				if selected {
					chosen = i + 1
				}
				b := ui.ButtonBase(c.Key(avatar)).Label(avatarName(c, avatar)).Size(avatarCell, avatarCell).
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
