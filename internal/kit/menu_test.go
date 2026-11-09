package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// The menu paints the floating color, highlights in accent.background
// (Palette.Hover), not the primary, and reports the item chosen.
func TestMenuLooksAndChooses(t *testing.T) {
	var p *theme.Palette
	got := ""
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		p = theme.Of(c)
		ui.Column(c).Fill().Padding(20).Children(func() {
			MenuButton(c, "test", func(b ui.Element) {
				GhostLook(c, b.Label("Open"), false).Children(func() { ui.Text(c, "Open") })
			}, func(m *Menu) {
				for _, o := range []string{"Alpha", "Beta"} {
					if m.Item(o).Checked(o == "Alpha").Shortcut("⌘1").Chosen() {
						got = o
					}
				}
			})
		})
	}, 400, 300)
	tt.Frame()
	if err := tt.Click("Open"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	r, ok := tt.Find("Beta")
	if !ok {
		t.Fatal("menu does not show")
	}
	at := func(x, y float32) ui.Color {
		c := tt.Image().RGBAAt(int(x), int(y))
		return ui.RGBA(c.R, c.G, c.B, 1)
	}
	near := func(a, b ui.Color) bool {
		d := func(x, y uint8) bool { return int(x)-int(y) <= 3 && int(y)-int(x) <= 3 }
		return d(a.R, b.R) && d(a.G, b.G) && d(a.B, b.B)
	}
	tt.Move(r.X+r.W/2, r.Y+r.H/2)
	tt.Frame()
	// The panel over the window, the highlight over the panel.
	blend := func(over, under ui.Color) ui.Color {
		a := float32(over.A) / 255
		mix := func(o, u uint8) uint8 { return uint8(float32(o)*a + float32(u)*(1-a) + 0.5) }
		return ui.RGBA(mix(over.R, under.R), mix(over.G, under.G), mix(over.B, under.B), 1)
	}
	panel := blend(p.Floating(), at(4, 4))
	if c := at(r.X+r.W/2, r.Y-1); !near(c, panel) {
		t.Errorf("panel %v, want floating %v", c, panel)
	}
	if c, want := at(r.X+r.W/2, r.Y+r.H/2), blend(p.Hover, panel); !near(c, want) {
		t.Errorf("highlight %v, want accent.background %v", c, want)
	}
	if err := tt.Click("Beta"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if got != "Beta" || tt.HasText("Alpha") {
		t.Errorf("chose %q, menu open %v", got, tt.HasText("Alpha"))
	}
}

// A select has a text input's height, and its dropdown is as wide as it.
func TestSelectMatchesInput(t *testing.T) {
	chosen, text := "Alpha", ""
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Padding(20).Gap(8).Children(func() {
			ui.TextInput(c.Key("input"), &text).Label("Name").Width(240)
			b, _ := Select(c.Key("select"), "test-select", "Pick", &chosen, []string{"Alpha", "Beta"}, false)
			b.Width(240)
		})
	}, 400, 400)
	tt.Frame()
	in, _ := tt.Access("Name")
	sel, ok := tt.Access("Pick")
	if !ok || in.Bounds.H != sel.Bounds.H {
		t.Fatalf("input %v, select %v", in.Bounds, sel.Bounds)
	}
	tt.ClickAt(sel.Bounds.X+10, sel.Bounds.Y+5)
	tt.Frame()
	tt.Frame()
	menu, ok := tt.AccessByRole(ui.RoleMenu, "")
	if !ok || menu.Bounds.W != sel.Bounds.W {
		t.Errorf("dropdown %v, select %v", menu.Bounds, sel.Bounds)
	}
}
