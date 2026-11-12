package settings

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
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
// read from the picture's file.
func avatarPixel(t *testing.T, name string, fx, fy float32) color.RGBA {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "icons", "avatars", name+".jpg"))
	if err != nil {
		t.Fatal(err)
	}
	picture, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	b := picture.Bounds()
	r, g, bl, _ := picture.At(b.Min.X+int(fx*float32(b.Dx())), b.Min.Y+int(fy*float32(b.Dy()))).RGBA()
	return color.RGBA{uint8(r >> 8), uint8(g >> 8), uint8(bl >> 8), 255}
}

// near reports whether two colors are the same within a JPEG's and a
// scaled picture's rounding.
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
	// tile is whether the Roles list draws the avatar's picture in the
	// row of the role named name: the color of the picture's top right,
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
			at := tt.Image().RGBAAt(int(x+0.85*kit.TileSize), int(title.Y+dy+0.2*kit.TileSize))
			if near(at, avatarPixel(t, avatar, 0.85, 0.2)) {
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
		if len(icons.AvatarNames) != 24 {
			t.Fatalf("%d avatars", len(icons.AvatarNames))
		}
		var first ui.Rect
		for i, name := range icons.AvatarNames {
			if icons.Avatar(icons.AvatarPrefix+name) == nil {
				t.Fatalf("no picture of %s", name)
			}
			cell, ok := tt.AccessByRole(ui.RoleButton, en("avatar_"+name))
			if !ok {
				t.Fatalf("no cell %q", en("avatar_"+name))
			}
			if i == 0 {
				first = cell.Bounds
			}
			want := ui.Rect{X: first.X + float32(i%6)*(avatarCell+avatarGap), Y: first.Y + float32(i/6)*(avatarCell+avatarGap), W: avatarCell, H: avatarCell}
			if cell.Bounds != want {
				t.Errorf("%s is at %v, row %d column %d is %v", name, cell.Bounds, i/6, i%6, want)
			}
		}
		none, ok := tt.AccessByRole(ui.RoleButton, en("role_avatar_none"))
		if !ok || none.Bounds.Y+none.Bounds.H > first.Y {
			t.Errorf("No avatar %v is not over the grid at %v", none.Bounds, first)
		}
	})

	t.Run("choose and save", func(t *testing.T) {
		m, tt := open(t, "tester")
		if tile(t, tt, "Tester", "whale") {
			t.Fatal("the row shows a picture before one is chosen")
		}
		click(t, tt, en("avatar_whale"))
		if m.roleEdit.role.Avatar != "avatar-whale" {
			t.Fatalf("the draft's avatar is %q", m.roleEdit.role.Avatar)
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, en("avatar_fox")); ok {
			t.Error("the picker stayed open")
		}
		if !tt.HasText(en("avatar_whale")) {
			t.Errorf("the field does not name the avatar: %q", tt.Texts())
		}
		if m.role("tester").Avatar != "" {
			t.Error("the role changed before Save")
		}
		click(t, tt, en("role_save"))
		if got := m.role("tester").Avatar; got != "avatar-whale" || m.roleEdit.open {
			t.Fatalf("saved avatar %q, editor open %v", got, m.roleEdit.open)
		}
		if !tile(t, tt, "Tester", "whale") {
			t.Error("the row does not draw the chosen picture")
		}
	})

	t.Run("no avatar", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		tt := page(m)
		if !tile(t, tt, "Reviewer", "panda") {
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
		if m.role("reviewer").Avatar != "" || tile(t, tt, "Reviewer", "panda") {
			t.Errorf("the row keeps the picture, avatar %q", m.role("reviewer").Avatar)
		}
	})

	t.Run("unknown", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.TeamTab = 1
		m.role("reviewer").Avatar = "avatar-unicorn"
		tt := page(m)
		if tile(t, tt, "Reviewer", "panda") || !tt.HasText("Reviewer") {
			t.Error("an unknown avatar does not show the role with its mark")
		}
		tt = ui.NewTester(func(c *ui.Context) {
			for _, avatar := range []string{"avatar-unicorn", "code", ""} {
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
		if !tt.Focused(en("avatar_owl")) {
			t.Fatalf("the keys did not reach %q", en("avatar_owl"))
		}
		tt.Key(0, ui.KeyEnter)
		settle(tt)
		if m.roleEdit.role.Avatar != "avatar-owl" {
			t.Fatalf("Enter chose %q", m.roleEdit.role.Avatar)
		}
		// Escape closes the picker and keeps the draft and the editor.
		click(t, tt, en("role_avatar"))
		if !tt.Focused(en("avatar_owl")) {
			t.Error("the picker does not open on the chosen avatar")
		}
		tt.Key(0, ui.KeyEscape)
		settle(tt)
		if _, ok := tt.AccessByRole(ui.RoleButton, en("avatar_fox")); ok || !m.roleEdit.open || m.roleEdit.role.Avatar != "avatar-owl" {
			t.Errorf("Escape: editor open %v, avatar %q", m.roleEdit.open, m.roleEdit.role.Avatar)
		}
	})

	t.Run("save action", func(t *testing.T) {
		m, tt := open(t, "tester")
		m.Live = true
		var saved []preview.Role
		m.TeamActions = &TeamActions{SaveRole: func(role preview.Role, done func(kit.Outcome)) { saved = append(saved, role) }}
		settle(tt)
		click(t, tt, en("avatar_fox"))
		click(t, tt, en("role_save"))
		if len(saved) != 1 || saved[0].Avatar != "avatar-fox" || saved[0].ID != "tester" {
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
					if cell, ok := tt.AccessByRole(ui.RoleButton, en("avatar_cat")); ok {
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
