package kit

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
	// A window that sweeps its menus each frame keeps the clicks of the frame
	// before, and a menu whose button left does not come back open with it.
	t.Run("swept", func(t *testing.T) {
		shown := true
		var states int
		tt := ui.NewTester(func(c *ui.Context) {
			SweepMenus(c)
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Padding(20).Children(func() {
				if shown {
					MenuButton(c, "swept", func(b ui.Element) {
						GhostLook(c, b.Label("Open"), false).Children(func() { ui.Text(c, "Open") })
					}, func(m *Menu) { m.Item("Alpha") })
				}
			})
			states = len(*ui.Local(c.Root(), menuOpen{}, func() map[string]*menuEntry { return nil }))
		}, 400, 300)
		tt.Frame()
		if err := tt.Click("Open"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(200 * time.Millisecond)
		if !tt.HasText("Alpha") {
			t.Fatal("the click did not open a swept menu")
		}
		shown = false
		tt.Frame()
		tt.Frame()
		if states != 0 {
			t.Errorf("%d menu states kept without a button", states)
		}
		shown = true
		tt.Advance(200 * time.Millisecond)
		if tt.HasText("Alpha") {
			t.Error("the menu came back open with its button")
		}
	})
	// A submenu opens from its item and its items are chosen as the menu's.
	t.Run("Submenu", func(t *testing.T) {
		got := ""
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Padding(20).Children(func() {
				MenuButton(c, "test", func(b ui.Element) {
					GhostLook(c, b.Label("Open"), false).Children(func() { ui.Text(c, "Open") })
				}, func(m *Menu) {
					if m.Item("Alpha").Chosen() {
						got = "Alpha"
					}
					m.Submenu("More", func(m *Menu) {
						for _, o := range []string{"Beta", "Gamma"} {
							if m.Item(o).Chosen() {
								got = o
							}
						}
					})
				})
			})
		}, 400, 300)
		tt.Frame()
		if err := tt.Click("Open"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(200 * time.Millisecond)
		r, ok := tt.Find("More")
		if !ok {
			t.Fatal("no submenu item")
		}
		tt.Move(r.X+r.W/2, r.Y+r.H/2)
		tt.Frame()
		tt.Advance(200 * time.Millisecond)
		if err := tt.Click("Gamma"); err != nil {
			t.Fatalf("the submenu does not open: %v %q", err, tt.Texts())
		}
		tt.Advance(200 * time.Millisecond)
		if got != "Gamma" || tt.HasText("Alpha") {
			t.Errorf("chose %q, menu open %v", got, tt.HasText("Alpha"))
		}
	})
	// An item's own button runs its action and closes the menu without
	// choosing the item.
	t.Run("item action", func(t *testing.T) {
		chosen, acted := false, 0
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Padding(20).Children(func() {
				MenuButton(c, "test", func(b ui.Element) {
					GhostLook(c, b.Label("Open"), false).Children(func() { ui.Text(c, "Open") })
				}, func(m *Menu) {
					if m.Item("Host").Action("Update", func() { acted++ }).Chosen() {
						chosen = true
					}
				})
			})
		}, 400, 300)
		tt.Frame()
		if err := tt.Click("Open"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(200 * time.Millisecond)
		b, ok := tt.Find("Update")
		if !ok {
			t.Fatalf("no action button: %q", tt.Texts())
		}
		tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		tt.Frame()
		tt.Advance(200 * time.Millisecond)
		if acted != 1 || chosen || tt.HasText("Host") {
			t.Errorf("the action ran %d times, chose the item %v, menu open %v", acted, chosen, tt.HasText("Host"))
		}
	})
	// An item with a description is a choice row: its icon in a tile of its
	// tint, the checked one without a fill, the highlight alone filled, a
	// warning's description in the warning color, and nothing redrawn at
	// rest.
	t.Run("choice rows", func(t *testing.T) {
		var p *theme.Palette
		tt := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			p = theme.Of(c)
			ui.Column(c).Fill().Padding(20).Children(func() {
				MenuButton(c, "choices", func(b ui.Element) {
					GhostLook(c, b.Label("Open"), false).Children(func() { ui.Text(c, "Open") })
				}, func(m *Menu) {
					m.Item("Alpha").Description("First").Icon(icons.Map).Tint(theme.Tints[0]).Checked(true).Chosen()
					m.Item("Beta").Description("Second").Icon(icons.Bot).Chosen()
					m.Item("Gamma").Description("Third").Icon(icons.TriangleAlert).Tint(theme.Tints[3]).Warning().Chosen()
				})
			})
		}, 400, 300, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.Frame()
		if err := tt.Click("Open"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(200 * time.Millisecond)
		item := func(name string) ui.AccessInfo {
			t.Helper()
			for _, e := range tt.AccessAll() {
				if e.Name == name && (e.Role == ui.RoleMenuItem || e.Role == ui.RoleMenuItemCheckBox) {
					return e
				}
			}
			t.Fatalf("no item %q: %q", name, tt.Texts())
			return ui.AccessInfo{}
		}
		at := func(x, y float32) ui.Color {
			c := tt.Image().RGBAAt(int(x), int(y))
			return ui.RGBA(c.R, c.G, c.B, 1)
		}
		// edge is a row's fill, in its padding before the tile.
		edge := func(name string) ui.Color {
			b := item(name).Bounds
			return at(b.X+3, b.Y+b.H/2)
		}
		// chroma is the most colored pixel of box: how far its channels are
		// apart, and whether it is warm (the warning color's hue).
		chroma := func(box ui.Rect) (most int, warm bool) {
			for y := int(box.Y); y < int(box.Y+box.H); y++ {
				for x := int(box.X); x < int(box.X+box.W); x++ {
					c := tt.Image().RGBAAt(x, y)
					r, g, b := int(c.R), int(c.G), int(c.B)
					if d := max(r, g, b) - min(r, g, b); d > most {
						most, warm = d, r-b > 60 && g-b > 30
					}
				}
			}
			return most, warm
		}
		tile := func(name string) ui.Rect {
			b := item(name).Bounds
			return ui.Rect{X: b.X + 8, Y: b.Y + (b.H-choiceTile)/2, W: choiceTile, H: choiceTile}
		}
		if alpha, beta := item("Alpha"), item("Beta"); !alpha.Checked || beta.Checked {
			t.Errorf("checked: Alpha %v, Beta %v", alpha.Checked, beta.Checked)
		}
		if got, rest := edge("Alpha"), edge("Beta"); !near(got, rest) {
			t.Errorf("the checked row is filled %v, a row at rest %v", got, rest)
		}
		if most, _ := chroma(tile("Alpha")); most < 60 {
			t.Errorf("the tinted tile's color is %d apart, want its tint", most)
		}
		if most, _ := chroma(tile("Beta")); most > 40 {
			t.Errorf("the tile without a tint is %d apart, want the neutral gray", most)
		}
		if _, warm := chroma(tile("Gamma")); !warm {
			t.Error("the amber tile is not warm")
		}
		for name, want := range map[string]bool{"Second": false, "Third": true} {
			r, ok := tt.Find(name)
			if !ok {
				t.Fatalf("no description %q", name)
			}
			if most, warm := chroma(r); (most > 60 && warm) != want {
				t.Errorf("description %q is %d apart, warm %v; in the warning color: want %v", name, most, warm, want)
			}
		}
		rest := edge("Beta")
		g := item("Gamma").Bounds
		tt.Move(g.X+g.W/2, g.Y+g.H/2)
		tt.Advance(200 * time.Millisecond)
		if got, want := edge("Gamma"), blend(p.Hover, rest); !near(got, want) {
			t.Errorf("the highlighted row is %v, want accent.background %v", got, want)
		}
		if got := edge("Alpha"); !near(got, rest) {
			t.Errorf("the checked row is filled %v beside the highlight", got)
		}
		tt.Advance(time.Second)
		if tt.FramePending() {
			t.Error("the open menu keeps redrawing at rest")
		}
	})
}

// The form controls line up with a text input.
func TestControlsMatchInput(t *testing.T) {
	// A select has a text input's height, and its dropdown is as wide as it.
	t.Run("Select", func(t *testing.T) {
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
	})
	// A Segmented is as high as a text input.
	t.Run("Segmented", func(t *testing.T) {
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
	})
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
	// The caller changes the list under a lifted row (a queued message is
	// sent while another is being moved): the row is followed by its key,
	// or put back when it left, and no index outlives the list.
	lifted := func(t *testing.T) (*[]string, *ui.Tester) {
		items := []string{"a", "b", "c"}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Padding(20).Children(func() {
				SortableList(c, &items, func(s string) string { return s }, SortableListProps{Handle: "Drag"}, func(s string, handle func()) {
					handle()
					ui.Text(c, s)
				})
			})
		}, 300, 300)
		tt.Frame()
		var handles []ui.AccessInfo
		for _, e := range tt.AccessAll() {
			if e.Name == "Drag" {
				handles = append(handles, e)
			}
		}
		if len(handles) != 3 {
			t.Fatalf("%d handles", len(handles))
		}
		b := handles[1].Bounds // b's
		tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		for _, k := range []ui.Key{ui.KeySpace, ui.KeyDown} {
			tt.Key(0, k)
			tt.Frame()
		}
		return &items, tt
	}
	drop := func(tt *ui.Tester) {
		tt.Frame()
		tt.Key(0, ui.KeySpace)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
	}
	t.Run("list shrinks before the lifted row", func(t *testing.T) {
		items, tt := lifted(t)
		*items = (*items)[1:]
		drop(tt)
		if !slices.Equal(*items, []string{"c", "b"}) {
			t.Errorf("items %v", *items)
		}
	})
	t.Run("list grows before the lifted row", func(t *testing.T) {
		items, tt := lifted(t)
		*items = append([]string{"x"}, *items...)
		drop(tt)
		if !slices.Equal(*items, []string{"x", "a", "c", "b"}) {
			t.Errorf("items %v", *items)
		}
	})
	t.Run("lifted row leaves", func(t *testing.T) {
		items, tt := lifted(t)
		*items = []string{"a", "c"}
		drop(tt)
		if !slices.Equal(*items, []string{"a", "c"}) {
			t.Errorf("items %v", *items)
		}
	})
	t.Run("list shrinks under a dragged row", func(t *testing.T) {
		items := []string{"a", "b", "c"}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Padding(20).Children(func() {
				SortableList(c, &items, func(s string) string { return s }, SortableListProps{Handle: "Drag"}, func(s string, handle func()) {
					handle()
					ui.Text(c, s)
				})
			})
		}, 300, 300)
		tt.Frame()
		var last ui.Rect
		for _, e := range tt.AccessAll() {
			if e.Name == "Drag" {
				last = e.Bounds // c's
			}
		}
		x, y := last.X+last.W/2, last.Y+last.H/2
		tt.Press(x, y)
		for i := 1; i <= 4; i++ {
			tt.Move(x, y+float32(i)*4)
			tt.Frame()
		}
		items = items[:1] // the dragged row and the one before it leave
		tt.Frame()
		tt.Release(x, y+16)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if !slices.Equal(items, []string{"a"}) {
			t.Errorf("items %v", items)
		}
	})
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
