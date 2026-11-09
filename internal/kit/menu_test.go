package kit

import (
	"testing"
	"time"

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
	tt.Advance(200 * time.Millisecond) // the panel's popup motion
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
	tt.Advance(200 * time.Millisecond) // the row's color transition
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
	tt.Advance(200 * time.Millisecond) // the panel's exit
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
	tt.Advance(200 * time.Millisecond) // the panel's popup motion
	menu, ok := tt.AccessByRole(ui.RoleMenu, "")
	if !ok || menu.Bounds.W != sel.Bounds.W {
		t.Errorf("dropdown %v, select %v", menu.Bounds, sel.Bounds)
	}
}

// A Segmented is as high as a text input.
func TestSegmentedMatchesInput(t *testing.T) {
	chosen, text := 0, ""
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(20).Gap(8).AlignItems(ui.Start).Children(func() {
			ui.TextInput(c.Key("input"), &text).Label("Name").Width(240)
			Segmented(c, "seg", &chosen, []string{"One", "Two"}, SegmentedProps{}).Label("Pick")
		})
	}, 400, 300)
	tt.Frame()
	in, _ := tt.Access("Name")
	seg, ok := tt.Access("Pick")
	if d := in.Bounds.H - seg.Bounds.H; !ok || d > 1 || d < -1 {
		t.Errorf("input %v, segmented %v", in.Bounds, seg.Bounds)
	}
}

// InputOTP keeps digits only, up to its length, and reports the full code.
func TestInputOTP(t *testing.T) {
	code, done := "", ""
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(20).Children(func() {
			InputOTP(c, "otp", &code, InputOTPProps{OnComplete: func(s string) { done = s }}).Label("Code")
		})
	}, 400, 200)
	tt.Frame()
	if err := tt.Click("Code"); err != nil {
		t.Fatal(err)
	}
	for _, r := range "12a3456789" {
		tt.Type(string(r))
		tt.Frame()
	}
	if code != "123456" || done != "123456" {
		t.Errorf("code %q, completed %q", code, done)
	}
}

// A SortableList moves a row from the keyboard and reports the move.
func TestSortableListKeyboard(t *testing.T) {
	items := []string{"a", "b", "c"}
	var moved [2]int
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(20).Children(func() {
			SortableList(c, &items, func(s string) string { return s }, SortableListProps{Handle: "Drag",
				OnMove: func(from, to int) { moved = [2]int{from, to} }}, func(s string, handle func()) {
				handle()
				ui.Text(c, s)
			})
		})
	}, 300, 300)
	tt.Frame()
	if err := tt.Click("Drag"); err != nil {
		t.Fatal(err)
	}
	for _, k := range []ui.Key{ui.KeySpace, ui.KeyDown, ui.KeySpace} {
		tt.Key(0, k)
		tt.Frame()
	}
	tt.Advance(time.Second)
	tt.Frame()
	if items[0] != "b" || items[1] != "a" || moved != [2]int{0, 1} {
		t.Errorf("items %v, moved %v", items, moved)
	}
}

// A Tooltip describes its anchor and shows, on its side, once the pointer
// rests on it.
func TestTooltipShowsAndDescribes(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(80).Children(func() {
			b := ui.ButtonBase(c.Key("b")).Label("Share").Size(40, 24)
			Tooltip(c, b, "Share the session", TooltipProps{Placement: Bottom, Hotkey: "⌘S"})
		})
	}, 400, 300)
	tt.Frame()
	b, ok := tt.Access("Share")
	if !ok || b.Description != "Share the session" {
		t.Fatalf("anchor %+v", b)
	}
	tt.Move(b.Bounds.X+10, b.Bounds.Y+10)
	tt.Advance(time.Second)
	r, ok := tt.Find("Share the session")
	if !ok || r.Y < b.Bounds.Y+b.Bounds.H || !tt.HasText("⌘S") {
		t.Errorf("tip %v below %v, shows %v", r, b.Bounds, ok)
	}
}
