package shell

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
)

// probe reads a composer's frame by pixel, at twice the size so a text's
// strokes keep their color.
type probe struct {
	t  *testing.T
	tt *ui.Tester
}

// at is the pixel at a point given in DIPs.
func (p probe) at(x, y float32) [3]int {
	img := p.tt.Image()
	c := img.RGBAAt(int(x*2), int(y*2))
	return [3]int{int(c.R), int(c.G), int(c.B)}
}

// same is whether two pixels are one color, within the rounding of a blend.
func same(a, b [3]int) bool {
	for i := range a {
		if d := a[i] - b[i]; d > 3 || d < -3 {
			return false
		}
	}
	return true
}

// color is the most colored pixel of box: how far apart its channels are,
// whether it is the warning color's hue and whether the danger color's.
func (p probe) color(box ui.Rect) (most int, warning, danger bool) {
	img := p.tt.Image()
	for y := int(box.Y * 2); y < int((box.Y+box.H)*2); y++ {
		for x := int(box.X * 2); x < int((box.X+box.W)*2); x++ {
			c := img.RGBAAt(x, y)
			r, g, b := int(c.R), int(c.G), int(c.B)
			if d := max(r, g, b) - min(r, g, b); d > most {
				most = d
				warning = r-b > 60 && g-b > 30
				danger = r-g > 60 && g-b < 30 && b-g < 30
			}
		}
	}
	return most, warning, danger
}

// row is the choice named name: a menu's item, or a button of the settings
// popover's page.
func (p probe) row(name string, roles ...ui.Role) ui.AccessInfo {
	p.t.Helper()
	for _, e := range p.tt.AccessAll() {
		for _, role := range roles {
			if e.Name == name && e.Role == role {
				return e
			}
		}
	}
	p.t.Fatalf("no row %q: %q", name, p.tt.Texts())
	return ui.AccessInfo{}
}

// edge is a row's fill, in its padding before the tile.
func (p probe) edge(e ui.AccessInfo) [3]int {
	return p.at(e.Bounds.X+3, e.Bounds.Y+e.Bounds.H/2)
}

// tile is the box of a row's tile: 28 across, 8 in, at the row's middle.
func tile(e ui.AccessInfo) ui.Rect {
	b := e.Bounds
	return ui.Rect{X: b.X + 8, Y: b.Y + (b.H-28)/2, W: 28, H: 28}
}

// hover rests the pointer on a row until its highlight has eased in.
func (p probe) hover(e ui.AccessInfo) {
	p.tt.Move(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
	p.tt.Frame()
	p.tt.Advance(300 * time.Millisecond)
	p.tt.Frame()
}

// composerMenus are the mode and permission menus' rows and the settings
// popover's, the same Kit choice row: the chosen one checked and unfilled,
// the fill the highlight's alone, Full access in the warning color on its
// description and on the control, and a package's mode in its key's tint.
// With SAILRY_SNAPSHOTS set the open permission menu is saved, dark and
// light, under .runtime/ui-shots.
func composerMenus(t *testing.T) {
	items := []ui.Role{ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio}
	ask, project, full := zh("composer_permission_ask"), zh("composer_permission_project"), zh("composer_permission_full")
	// menuRow is a menu row's height, which the popover's rows share.
	var menuRow float32

	for _, mode := range []string{"dark", "light"} {
		t.Run("permission menu "+mode, func(t *testing.T) {
			tt, comp, _ := composerWith(t, 960, nil)
			comp.Permission = composer.FullAccess
			tt.SetDark(mode == "dark")
			tt.SetScale(2)
			tt.Frame()
			tt.Frame()
			p := probe{t, tt}
			trigger, ok := control(tt, full)
			if !ok {
				t.Fatalf("no permission control: %q", tt.Texts())
			}
			if most, warning, danger := p.color(trigger.Bounds); most < 60 || !warning || danger {
				t.Errorf("the Full access control is %d apart, warning %v, danger %v: want the warning color", most, warning, danger)
			}
			pressButton(t, tt, full)
			tt.Advance(300 * time.Millisecond)
			tt.Frame()
			chosen, rest, other := p.row(full, items...), p.row(ask, items...), p.row(project, items...)
			if !chosen.Checked || rest.Checked || other.Checked {
				t.Errorf("checked: full %v, ask %v, project %v", chosen.Checked, rest.Checked, other.Checked)
			}
			if got, want := p.edge(chosen), p.edge(rest); !same(got, want) {
				t.Errorf("the chosen row is filled %v, a row at rest %v", got, want)
			}
			description, ok := tt.Find(zh("composer_permission_full_description"))
			if !ok {
				t.Fatalf("no Full access description: %q", tt.Texts())
			}
			if most, warning, danger := p.color(description); most < 60 || !warning || danger {
				t.Errorf("the Full access description is %d apart, warning %v, danger %v: want the warning color", most, warning, danger)
			}
			if plain, ok := tt.Find(zh("composer_permission_ask_description")); !ok {
				t.Errorf("no description of asking")
			} else if most, _, _ := p.color(plain); most > 40 {
				t.Errorf("the description of asking is %d apart, want the muted color", most)
			}
			p.hover(other)
			if got, want := p.edge(other), p.edge(rest); same(got, want) {
				t.Errorf("the row under the pointer has no fill: %v", got)
			}
			if got, want := p.edge(chosen), p.edge(rest); !same(got, want) {
				t.Errorf("the chosen row is filled %v beside the highlight", got)
			}
			menuRow = rest.Bounds.H
			if os.Getenv("SAILRY_SNAPSHOTS") == "" {
				return
			}
			shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
			if err := os.MkdirAll(shots, 0o755); err != nil {
				t.Fatal(err)
			}
			file, err := os.Create(filepath.Join(shots, "composer-menus-zh-CN-"+mode+".png"))
			if err != nil {
				t.Fatal(err)
			}
			defer file.Close()
			if err := png.Encode(file, tt.Image()); err != nil {
				t.Fatal(err)
			}
		})
	}

	// A package's mode has a tile in a color of its own, the same in every
	// window; the built-in Code mode's is the neutral gray.
	t.Run("package mode tint", func(t *testing.T) {
		tinted := func() (most int, pixel [3]int) {
			tt, _, _ := composerWith(t, 960, func(_ *composer.State, ctx *composer.Context) {
				ctx.PluginModes = []composer.PluginMode{{Key: "goals/goal", Label: "目标模式", Description: "按目标持续推进", Icon: icons.Tasks}}
				ctx.SelectMode = func(string) {}
			})
			tt.SetScale(2)
			tt.Frame()
			p := probe{t, tt}
			pressButton(t, tt, zh("composer_mode_code"))
			tt.Advance(300 * time.Millisecond)
			tt.Frame()
			if most, _, _ := p.color(tile(p.row(zh("composer_mode_code"), items...))); most > 40 {
				t.Errorf("the Code mode's tile is %d apart, want the neutral gray", most)
			}
			box := tile(p.row("目标模式", items...))
			most, _, _ = p.color(box)
			return most, p.at(box.X+2, box.Y+box.H/2)
		}
		most, first := tinted()
		if most < 60 {
			t.Errorf("the package mode's tile is %d apart, want a tint", most)
		}
		if _, again := tinted(); again != first {
			t.Errorf("the package mode's tile is %v, then %v: want one tint", first, again)
		}
	})

	// The compact settings popover's permission page has the menu's rows.
	t.Run("settings popover", func(t *testing.T) {
		tt, comp, _ := composerWith(t, 440, nil)
		comp.Permission = composer.FullAccess
		tt.SetScale(2)
		tt.Frame()
		tt.Frame()
		p := probe{t, tt}
		opened := false
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && strings.HasPrefix(e.Name, zh("composer_settings")) {
				tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
				opened = true
				break
			}
		}
		if !opened {
			t.Fatalf("no settings button at the compact width: %q", tt.Texts())
		}
		tt.Frame()
		// The popover fades in: read it once it rests.
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		// The permission's row, in the warning color, opens its page.
		page := p.row(full, ui.RoleButton)
		if most, warning, danger := p.color(page.Bounds); most < 60 || !warning || danger {
			t.Errorf("the popover's Full access row is %d apart, warning %v, danger %v: want the warning color", most, warning, danger)
		}
		tt.ClickAt(page.Bounds.X+page.Bounds.W/2, page.Bounds.Y+page.Bounds.H/2)
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		chosen, rest, other := p.row(full, ui.RoleButton), p.row(ask, ui.RoleButton), p.row(project, ui.RoleButton)
		// A choice row reports its choice as checked.
		if !chosen.Checked || rest.Checked {
			t.Errorf("checked: full %v, ask %v", chosen.Checked, rest.Checked)
		}
		if menuRow != 0 && rest.Bounds.H != menuRow {
			t.Errorf("a popover row is %v high, the menu's %v", rest.Bounds.H, menuRow)
		}
		// The pointer is where the page's row was, which may now be one of
		// these: it leaves before the rows are read at rest.
		tt.Move(2, 2)
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		if got, want := p.edge(chosen), p.edge(rest); !same(got, want) {
			t.Errorf("the chosen row is filled %v, a row at rest %v", got, want)
		}
		if most, _, _ := p.color(tile(chosen)); most < 60 {
			t.Errorf("the chosen row's tile is %d apart, want its amber tint", most)
		}
		if most, _, _ := p.color(tile(rest)); most > 40 {
			t.Errorf("the tile of asking is %d apart, want the neutral gray", most)
		}
		description, ok := tt.Find(zh("composer_permission_full_description"))
		if !ok {
			t.Fatalf("no Full access description: %q", tt.Texts())
		}
		if most, warning, danger := p.color(description); most < 60 || !warning || danger {
			t.Errorf("the Full access description is %d apart, warning %v, danger %v: want the warning color", most, warning, danger)
		}
		p.hover(other)
		if got, want := p.edge(other), p.edge(rest); same(got, want) {
			t.Errorf("the row under the pointer has no fill: %v", got)
		}
	})
}
