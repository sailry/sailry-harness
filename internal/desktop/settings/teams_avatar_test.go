package settings

import (
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// avatarPixel is the color of an avatar's picture at (fx, fy) of its side,
// drawn on its own.
func avatarPixel(t *testing.T, avatar string, fx, fy float32) color.RGBA {
	t.Helper()
	picture := icons.Avatar(avatar)
	if picture == nil {
		t.Fatalf("no picture of %q", avatar)
	}
	const side = 192
	return picture.Image(side, side, nil).RGBAAt(int(fx*side), int(fy*side))
}

// near reports whether two colors are the same within a scaled picture's
// rounding.
func near(a, b color.RGBA) bool {
	d := func(x, y uint8) int { return max(int(x)-int(y), int(y)-int(x)) }
	return d(a.R, b.R) < 24 && d(a.G, b.G) < 24 && d(a.B, b.B) < 24
}

// The role editor's avatar picker, and the avatar on the Roles list.
func TestRoleAvatars(t *testing.T) {
	en := func(key string) string { return i18n.In("en", key) }
	page := func(m *Model) *ui.Tester {
		tt := ui.NewTesterWith(func(c *ui.Context) { teams(c, m) }, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		return tt
	}
	settle := func(tt *ui.Tester) {
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
	}
	click := func(t *testing.T, tt *ui.Tester, name string) {
		t.Helper()
		b, ok := tt.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no button %q", name)
		}
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		settle(tt)
	}
	// cells are the picker's pictures, in the grid's order.
	cells := func(tt *ui.Tester) []ui.AccessInfo {
		prefix := strings.TrimSuffix(en("role_avatar_seed"), "%{seed}")
		var found []ui.AccessInfo
		for _, a := range tt.AccessAll() {
			if a.Role == ui.RoleButton && strings.HasPrefix(a.Name, prefix) {
				found = append(found, a)
			}
		}
		return found
	}
	name := func(avatar string) string {
		return i18n.In("en", "role_avatar_seed", "seed", strings.TrimPrefix(avatar, icons.AvatarPrefix))
	}
	// tile is whether the Roles list draws the avatar's picture in the
	// row of the role named name: the color of the picture's left middle,
	// which is its plain ground, is there in the row's tile.
	tile := func(t *testing.T, tt *ui.Tester, name, avatar string) bool {
		t.Helper()
		title, ok := tt.Find(name)
		if !ok {
			t.Fatalf("no row %q: %q", name, tt.Texts())
		}
		// MediaRow: the tile's slot, a gap of 12, then the title, whose
		// line starts within a few DIPs of the tile's top.
		x := title.X - 12 - kit.TileSize
		for dy := float32(-6); dy <= 6; dy += 2 {
			at := tt.Image().RGBAAt(int(x+0.08*kit.TileSize), int(title.Y+dy+0.5*kit.TileSize))
			if near(at, avatarPixel(t, avatar, 0.08, 0.5)) {
				return true
			}
		}
		return false
	}
	open := func(t *testing.T, id string) (*Model, *ui.Tester) {
		t.Helper()
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		m.roleEdit = roleEditor{open: true, role: *m.role(id)}
		tt := page(m)
		click(t, tt, en("role_avatar"))
		return m, tt
	}

	t.Run("grid", func(t *testing.T) {
		_, tt := open(t, "tester")
		grid := cells(tt)
		if len(grid) != avatarColumns*avatarRows {
			t.Fatalf("%d pictures", len(grid))
		}
		seen := map[string]bool{}
		for i, cell := range grid {
			if seen[cell.Name] {
				t.Errorf("%q is offered twice", cell.Name)
			}
			seen[cell.Name] = true
			first := grid[0].Bounds
			want := ui.Rect{X: first.X + float32(i%6)*(avatarCell+avatarGap), Y: first.Y + float32(i/6)*(avatarCell+avatarGap), W: avatarCell, H: avatarCell}
			if cell.Bounds != want {
				t.Errorf("%s is at %v, row %d column %d is %v", cell.Name, cell.Bounds, i/6, i%6, want)
			}
		}
		none, ok := tt.AccessByRole(ui.RoleButton, en("role_avatar_none"))
		if !ok || none.Bounds.Y+none.Bounds.H > grid[0].Bounds.Y {
			t.Errorf("No avatar %v is not over the grid at %v", none.Bounds, grid[0].Bounds)
		}
		// Shuffle deals new pictures.
		click(t, tt, en("role_avatar_shuffle"))
		again := cells(tt)
		if len(again) != len(grid) || again[0].Name == grid[0].Name && again[1].Name == grid[1].Name {
			t.Errorf("Shuffle kept %q", again[0].Name)
		}
	})

	t.Run("chosen first", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		m.roleEdit = roleEditor{open: true, role: *m.role("reviewer")}
		avatar := m.roleEdit.role.Avatar
		tt := page(m)
		click(t, tt, en("role_avatar"))
		click(t, tt, en("role_avatar_shuffle"))
		if grid := cells(tt); len(grid) == 0 || grid[0].Name != name(avatar) {
			t.Fatalf("the grid does not open on %q: %v", name(avatar), grid)
		}
	})

	t.Run("choose and save", func(t *testing.T) {
		m, tt := open(t, "tester")
		pick := cells(tt)[2]
		avatar := icons.AvatarPrefix + strings.TrimPrefix(pick.Name, strings.TrimSuffix(en("role_avatar_seed"), "%{seed}"))
		if tile(t, tt, "Tester", avatar) {
			t.Fatal("the row shows a picture before one is chosen")
		}
		click(t, tt, pick.Name)
		if m.roleEdit.role.Avatar != avatar {
			t.Fatalf("the draft's avatar is %q, want %q", m.roleEdit.role.Avatar, avatar)
		}
		if len(cells(tt)) != 0 {
			t.Error("the picker stayed open")
		}
		if !tt.HasText(name(avatar)) {
			t.Errorf("the field does not name the avatar: %q", tt.Texts())
		}
		if m.role("tester").Avatar != "" {
			t.Error("the role changed before Save")
		}
		click(t, tt, en("role_save"))
		if got := m.role("tester").Avatar; got != avatar || m.roleEdit.open {
			t.Fatalf("saved avatar %q, editor open %v", got, m.roleEdit.open)
		}
		if !tile(t, tt, "Tester", avatar) {
			t.Error("the row does not draw the chosen picture")
		}
	})

	t.Run("no avatar", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		tt := page(m)
		if !tile(t, tt, "Reviewer", "avatar-panda") {
			t.Fatal("the row does not draw the role's picture")
		}
		m.roleEdit = roleEditor{open: true, role: *m.role("reviewer")}
		settle(tt)
		click(t, tt, en("role_avatar"))
		click(t, tt, en("role_avatar_none"))
		if m.roleEdit.role.Avatar != "" {
			t.Fatalf("the draft's avatar is %q", m.roleEdit.role.Avatar)
		}
		click(t, tt, en("role_save"))
		if m.role("reviewer").Avatar != "" || tile(t, tt, "Reviewer", "avatar-panda") {
			t.Errorf("the row keeps the picture, avatar %q", m.role("reviewer").Avatar)
		}
	})

	t.Run("unknown", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		m.role("reviewer").Avatar = "avatar-Unicorn"
		tt := page(m)
		if tile(t, tt, "Reviewer", "avatar-panda") || !tt.HasText("Reviewer") {
			t.Error("an unknown avatar does not show the role with its mark")
		}
		tt = ui.NewTester(func(c *ui.Context) {
			for _, avatar := range []string{"avatar-Unicorn", "avatar-", "code", ""} {
				if _, drawn := kit.Avatar(c, avatar, 36); drawn {
					t.Errorf("%q has a picture", avatar)
				}
			}
			if _, drawn := kit.Avatar(c, "avatar-fox", 36); !drawn {
				t.Error("avatar-fox has no picture")
			}
		}, 100, 100)
		tt.Frame()
	})

	t.Run("keyboard", func(t *testing.T) {
		m, tt := open(t, "tester")
		// It opens on what is chosen, No avatar: Down is the first picture,
		// Right the second, Down the one under it, Up and Down that again.
		for _, key := range []ui.Key{ui.KeyDown, ui.KeyRight, ui.KeyDown, ui.KeyUp, ui.KeyDown} {
			tt.Key(0, key)
			settle(tt)
		}
		target := cells(tt)[1+avatarColumns].Name
		if !tt.Focused(target) {
			t.Fatalf("the keys did not reach %q", target)
		}
		tt.Key(0, ui.KeyEnter)
		settle(tt)
		if name(m.roleEdit.role.Avatar) != target {
			t.Fatalf("Enter chose %q, want %q", m.roleEdit.role.Avatar, target)
		}
		// It opens again on the chosen avatar, first in the grid.
		click(t, tt, en("role_avatar"))
		if grid := cells(tt); grid[0].Name != target || !tt.Focused(target) {
			t.Error("the picker does not open on the chosen avatar")
		}
		// Escape closes the picker and keeps the draft and the editor.
		chosen := m.roleEdit.role.Avatar
		tt.Key(0, ui.KeyEscape)
		settle(tt)
		if len(cells(tt)) != 0 || !m.roleEdit.open || m.roleEdit.role.Avatar != chosen {
			t.Errorf("Escape: editor open %v, avatar %q", m.roleEdit.open, m.roleEdit.role.Avatar)
		}
	})

	t.Run("save action", func(t *testing.T) {
		m, tt := open(t, "tester")
		m.Live = true
		var saved []preview.Role
		m.TeamActions = &TeamActions{SaveRole: func(role preview.Role, done func(kit.Outcome)) { saved = append(saved, role) }}
		settle(tt)
		pick := cells(tt)[0].Name
		click(t, tt, pick)
		click(t, tt, en("role_save"))
		if len(saved) != 1 || name(saved[0].Avatar) != pick || saved[0].ID != "tester" {
			t.Fatalf("saved %+v", saved)
		}
	})

	// For review: $SAILRY_SNAPSHOTS/roles-<list|picker>-<mode>.png at 2x.
	t.Run("snapshots", func(t *testing.T) {
		dir := os.Getenv("SAILRY_SNAPSHOTS")
		if dir == "" {
			t.Skip("SAILRY_SNAPSHOTS is not set")
		}
		for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
			name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
			for _, shot := range []string{"list", "picker"} {
				m := NewModel(preview.SettingsData())
				m.TeamTab = 1
				m.role("tester").Avatar = "avatar-whale"
				m.Roles = append(m.Roles, preview.Role{ID: "writer", Key: "writer", Name: "Writer", Description: "Write the docs"})
				if shot == "picker" {
					m.roleEdit = roleEditor{open: true, role: *m.role("tester")}
				}
				tt := ui.NewTesterWith(func(c *ui.Context) {
					theme.Apply(c, "", mode)
					ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() { teams(c, m) })
				}, 820, 760, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
				tt.SetLocale("en")
				tt.SetScale(2)
				settle(tt)
				if shot == "picker" {
					click(t, tt, en("role_avatar"))
					if grid := cells(tt); len(grid) > 3 {
						cell := grid[3]
						tt.Move(cell.Bounds.X+cell.Bounds.W/2, cell.Bounds.Y+cell.Bounds.H/2)
						settle(tt)
					}
				}
				out, err := os.Create(filepath.Join(dir, "roles-"+shot+"-"+name+".png"))
				if err != nil {
					t.Fatal(err)
				}
				if err := png.Encode(out, tt.Image()); err != nil {
					t.Fatal(err)
				}
				out.Close()
			}
		}
	})
}
