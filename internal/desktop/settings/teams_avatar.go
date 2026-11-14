package settings

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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

// avatarKind is what an avatar picker chooses for: a role's faces or a
// team's pictures. owner is the role's or team's ID, which picks the
// default; draw draws an avatar, or the default for ""; known reports
// whether an identifier has a picture of this kind.
type avatarKind struct {
	owner string
	draw  func(c *ui.Context, avatar string, size float32) ui.Element
	known func(avatar string) bool
}

// roleAvatars chooses a role's face (kit.RoleAvatar).
func roleAvatars(id string) avatarKind {
	return avatarKind{owner: id, known: func(avatar string) bool { return icons.Avatar(avatar) != nil },
		draw: func(c *ui.Context, avatar string, size float32) ui.Element {
			return kit.RoleAvatar(c, id, avatar, size)
		}}
}

// teamAvatars chooses a team's picture (kit.TeamAvatar).
func teamAvatars(id string) avatarKind {
	return avatarKind{owner: id, known: func(avatar string) bool { return icons.TeamAvatar(avatar) != nil },
		draw: func(c *ui.Context, avatar string, size float32) ui.Element {
			return kit.TeamAvatar(c, id, avatar, size)
		}}
}

// avatarPicker is an editor's avatar popover: whether it shows,
// enter, which gives the chosen avatar the focus as it opens, and the
// avatars it offers.
type avatarPicker struct {
	open, enter bool
	choices     []string
}

// deal fills the grid with new random avatars after the chosen one, which
// stays first so the grid always shows it.
func (st *avatarPicker) deal(kind avatarKind, chosen string) {
	st.choices = st.choices[:0]
	if kind.known(chosen) {
		st.choices = append(st.choices, chosen)
	}
	for len(st.choices) < avatarColumns*avatarRows {
		if avatar := icons.RandomAvatar(); !slices.Contains(st.choices, avatar) {
			st.choices = append(st.choices, avatar)
		}
	}
}

// avatarName is what assistive technology calls an avatar: "Avatar" and
// its seed, or "Default" for a role that shows the default face.
func avatarName(c *ui.Context, avatar string) string {
	if seed, ok := protocol.RoleAvatarSeed(avatar); ok {
		return i18n.T(c, "role_avatar_seed", "seed", seed)
	}
	return i18n.T(c, "role_avatar_none")
}

// avatarTrigger is the side of the role editor's avatar button.
const avatarTrigger = 48

// avatarField is an editor's Avatar row: the avatar as it looks now,
// avatarTrigger across, with a pencil badge, that opens the pictures of
// kind to choose from. Choosing one sets *avatar, the draft's, and closes
// the popover; the editor's Save stores it with the rest.
func avatarField(c *ui.Context, kind avatarKind, avatar *string, disabled bool) {
	p := theme.Of(c)
	label := i18n.T(c, "role_avatar")
	kit.Field(c, "avatar", label, "", func() {
		// Room for the badge, which reaches past the picture's corner.
		holder := ui.Row(c.Key("holder")).Padding(0, 4, 4, 0)
		st := ui.Local(holder, "picker", func() avatarPicker { return avatarPicker{} })
		holder.Children(func() {
			b := ui.ButtonBase(c.Key("trigger")).Label(label).Disabled(disabled).HasPopup(ui.PopupDialog).Expanded(st.open)
			b.Size(avatarTrigger, avatarTrigger).Radius(avatarTrigger / 4).Cursor(ui.CursorPointer)
			if b.IsDisabled() {
				b.Opacity(0.5)
			}
			b.Children(func() {
				kind.draw(c, *avatar, avatarTrigger)
				if ring := avatarRing(c, b, st.open); ring != (ui.Color{}) {
					// Inside the picture's edge: the editor's scroll area
					// clips anything past the field's start.
					ui.Box(c).Absolute().Top(0).Left(0).Size(avatarTrigger, avatarTrigger).Radius(avatarTrigger/4).Border(2, ring)
				}
				// The badge says the picture changes; it sits on the
				// corner, ringed in the dialog's color.
				ui.Box(c).Absolute().Right(-4).Bottom(-4).Size(18, 18).Radius(9).Center().
					Background(p.Floating()).Border(1, p.Border).TextColor(p.MutedForeground).Children(func() {
					ui.Icon(c, icons.Pencil).FontSize(10)
				})
			}).OnClick(func() {
				if st.open, st.enter = !st.open, true; st.open {
					st.deal(kind, *avatar)
				}
			})
			panel := kit.Popover(c, b, &st.open, func() { avatarChoices(c, st, kind, avatar) })
			if panel != (ui.Element{}) {
				panel.AttachTo(b, ui.AnchorBottomStart, ui.AnchorTopStart).Padding(12)
			}
		})
	})
}

// avatarRing is the ring around a picture button: the emphasis color
// while chosen, a faint one under the pointer, none otherwise.
func avatarRing(c *ui.Context, b ui.Element, chosen bool) ui.Color {
	p := theme.Of(c)
	switch {
	case chosen:
		return p.Primary
	case b.Hovered() || b.Pressed():
		return p.Foreground.Alpha(0.35)
	}
	return ui.Color{}
}

// avatarChoices is the popover's content: Default, which gives the role
// or team its default again, and Shuffle, which deals new random pictures, over the
// pictures in rows of avatarColumns. The chosen
// picture has a ring in the emphasis color around it, since a fill would
// be under the picture; the one under the pointer has a faint one. Left
// and Right move through the choices, Up and Down by a row, with No avatar
// above the first; Enter or Space chooses.
func avatarChoices(c *ui.Context, st *avatarPicker, kind avatarKind, current *string) {
	p := theme.Of(c)
	choose := func(avatar string) { *current, st.open = avatar, false }
	// The choices in the keys' order: No avatar, then the pictures.
	cells := make([]ui.Element, 0, 1+len(st.choices))
	chosen := 0
	body := ui.Column(c.Key("avatars")).Gap(avatarGap).FocusGroup(ui.Horizontal)
	body.Children(func() {
		ui.Row(c.Key("head")).Gap(avatarGap).AlignItems(ui.Center).Children(func() {
			name := i18n.T(c, "role_avatar_none")
			none := kit.Ghost(c.Key("none"), name, false).Grow(1).Height(32).PaddingX(8).Gap(8).Justify(ui.Start).
				AccessSelected(!kind.known(*current))
			if !kind.known(*current) {
				none.Background(p.Selected())
			}
			none.Children(func() {
				kind.draw(c, "", 20)
				ui.Text(c, name).FontSize(14).SingleLine()
			}).OnClick(func() { choose("") })
			cells = append(cells, none)
			kit.IconButton(c.Key("shuffle"), icons.Shuffle, i18n.T(c, "role_avatar_shuffle"), 32, false).
				OnClick(func() { st.deal(kind, *current) })
		})
		width := float32(avatarColumns*avatarCell + (avatarColumns-1)*avatarGap)
		ui.Grid(c.Key("grid")).Columns(avatarColumns).Gap(avatarGap).Width(width).Children(func() {
			for i, avatar := range st.choices {
				selected := *current == avatar
				if selected {
					chosen = i + 1
				}
				b := ui.ButtonBase(c.Key(avatar)).Label(avatarName(c, avatar)).Size(avatarCell, avatarCell).
					Radius(avatarCell / 4).Cursor(ui.CursorPointer).AccessSelected(selected)
				ring := avatarRing(c, b, selected)
				b.Children(func() {
					kind.draw(c, avatar, avatarCell)
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
