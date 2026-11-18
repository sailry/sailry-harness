package composer_test

import (
	"image"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func overviewSectionTester(s *composer.OverviewState, o *composer.Overview) *ui.Tester {
	var hidden bool
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Padding(24).AlignItems(ui.End).Background(theme.Of(c).Background).Children(func() {
			if s.Docked {
				composer.OverviewCard(c, s, o, 740)
			} else {
				composer.OverviewButton(c, s, o, 32, &hidden)
			}
		})
	}, 420, 870)
	tt.SetLocale("en")
	tt.Frame()
	return tt
}

func TestOverviewSectionDisclosure(t *testing.T) {
	const step, child, first, last = "Check the overview", "Review agent", "First artifact", "Last artifact"
	en := func(key string) string { return i18n.In("en", key) }
	status := en("chat_running")
	summary := i18n.In("en", "subagent_count", "completed", 1, "total", 1)
	all := en("chat_assets_all") + " 4"
	sections := []struct{ heading, row string }{
		{en("chat_assets_progress"), step},
		{en("chat_assets_activity"), summary},
		{en("chat_assets_changes"), "fixture.go"},
		{en("chat_assets_output"), first},
		{en("chat_assets_sources"), "Example source"},
	}
	for _, tc := range []struct {
		name   string
		docked bool
	}{{"card", true}, {"popover", false}} {
		t.Run(tc.name, func(t *testing.T) {
			s := &composer.OverviewState{Docked: tc.docked}
			popover := !tc.docked
			var opened []bool
			var asset string
			o := &composer.Overview{
				Session: true, Turn: true, Status: conversation.Running, Elapsed: 5 * time.Second, Doing: "Reading fixture",
				Steps:    []conversation.Step{{Text: step, State: conversation.StepCompleted}, {Text: "Apply changes"}},
				Children: []composer.OverviewChild{{ID: "child", Name: child, Completed: true, Status: "turn_completed"}},
				Changes:  []composer.OverviewChange{{Path: "fixture.go", Added: 7, Removed: 3}},
				Output: []composer.OverviewAsset{
					{ID: "first", Name: first, Icon: icons.File},
					{ID: "second", Name: "Second artifact", Icon: icons.File},
					{ID: "third", Name: "Third artifact", Icon: icons.File},
					{ID: "last", Name: last, Icon: icons.File},
				},
				Citations: []conversation.Citation{{Title: "Example source", URI: "https://example.com"}},
				Opened:    func(open bool) { opened = append(opened, open) },
				OpenAsset: func(id string) { asset = id },
			}
			tt := overviewSectionTester(s, o)
			click := func(label string) {
				t.Helper()
				if err := tt.Click(label); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
			}
			if popover {
				click(en("chat_assets_title"))
				tt.Advance(200 * time.Millisecond)
			}
			check := func(folded string) {
				t.Helper()
				for _, section := range sections {
					want := section.heading != folded
					h, ok := tt.AccessByRole(ui.RoleButton, section.heading)
					_, row := tt.Access(section.row)
					if !ok || !h.Focusable || h.Expanded != want || row != want {
						t.Fatalf("folded %q: heading %+v (found %v), row %q present %v", folded, h, ok, section.row, row)
					}
				}
				for _, text := range []string{status, "5s", "Reading fixture", "1/2", "+7", "−3"} {
					if _, ok := tt.Access(text); !ok {
						t.Errorf("folded %q: lost status or tally %q", folded, text)
					}
				}
				if _, ok := tt.AccessByRole(ui.RoleButton, status); ok {
					t.Error("the current run status became a disclosure")
				}
				if s.Open() != popover || !slices.Equal(opened, []bool{true}) {
					t.Fatalf("folded %q: visibility changed: open=%v, notifications=%v", folded, s.Open(), opened)
				}
			}
			check("")
			for _, section := range sections {
				click(section.heading)
				check(section.heading)
				click(section.heading)
				check("")
			}

			if tt.HasText(last) || tt.HasText(child) {
				t.Fatalf("the cap or subagent summary was bypassed: %q", tt.Texts())
			}
			click(all)
			click(summary)
			for _, item := range []struct{ heading, row, other string }{
				{en("chat_assets_output"), last, child},
				{en("chat_assets_activity"), child, last},
			} {
				click(item.heading)
				if tt.HasText(item.row) || !tt.HasText(item.other) {
					t.Fatalf("folding %q did not hide only its rows: %q", item.heading, tt.Texts())
				}
				click(item.heading)
				if !tt.HasText(item.row) || tt.HasText(all) || tt.HasText(summary) {
					t.Fatalf("reopening %q lost its list expansion: %q", item.heading, tt.Texts())
				}
			}

			output, sources := en("chat_assets_output"), en("chat_assets_sources")
			click(output)
			tt.Key(0, ui.KeyTab)
			if !tt.Focused(sources) {
				t.Fatal("Tab did not skip the folded output rows")
			}
			tt.Key(ui.Shift, ui.KeyTab)
			for _, key := range []ui.Key{ui.KeyEnter, ui.KeySpace, ui.KeyEnter} {
				before, _ := tt.AccessByRole(ui.RoleButton, output)
				tt.Key(0, key)
				after, _ := tt.AccessByRole(ui.RoleButton, output)
				if !after.Focused || after.Expanded == before.Expanded || tt.HasText(last) != after.Expanded || s.Open() != popover {
					t.Fatalf("keyboard disclosure lost focus, rows or visibility: %+v -> %+v", before, after)
				}
			}
			click(first)
			want := []bool{true}
			if popover {
				want = append(want, false)
			}
			if asset != "first" || s.Open() || !slices.Equal(opened, want) {
				t.Fatalf("opening an asset: id=%q, open=%v, notifications=%v", asset, s.Open(), opened)
			}
		})
	}
}

func TestOverviewSectionChevron(t *testing.T) {
	o := &composer.Overview{Session: true, Steps: []conversation.Step{{Text: "Verify layout", State: conversation.StepCompleted}}}
	tt := overviewSectionTester(&composer.OverviewState{Docked: true}, o)
	const scale = 2
	tt.SetScale(scale)
	title := i18n.In("en", "chat_assets_progress")
	h, ok := tt.AccessByRole(ui.RoleButton, title)
	tally, counted := tt.Find("1/1")
	if !ok || !counted {
		t.Fatalf("no progress heading or tally: %q", tt.Texts())
	}
	r := h.Bounds
	// Measure the ink in the slot after the title rather than comparing full snapshots.
	ink := func() image.Rectangle {
		t.Helper()
		current, _ := tt.AccessByRole(ui.RoleButton, title)
		count, _ := tt.Find("1/1")
		if current.Bounds != r || count != tally {
			t.Fatalf("the title or tally moved: %v -> %v, %v -> %v", r, current.Bounds, tally, count)
		}
		img := tt.Image()
		const slot = 14
		x0, y0 := int(scale*(r.X+r.W-slot)), int(scale*(r.Y+(r.H-slot)/2))
		bg := int(img.RGBAAt(x0, y0).R)
		var drawn image.Rectangle
		for y := y0; y < y0+scale*slot; y++ {
			for x := x0; x < x0+scale*slot; x++ {
				if int(img.RGBAAt(x, y).R)-bg > 24 {
					drawn = drawn.Union(image.Rect(x, y, x+1, y+1))
				}
			}
		}
		return drawn
	}
	if got := ink(); !got.Empty() {
		t.Fatalf("the expanded chevron is visible without hover: %v", got)
	}
	tt.Move(r.X+r.W/2, r.Y+r.H/2)
	if got := ink(); got.Dx() <= got.Dy() {
		t.Fatalf("hovering the expanded heading did not show a down chevron: %v", got)
	}
	tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	if got := ink(); got.Dy() <= got.Dx() {
		t.Fatalf("folding under hover did not show a right chevron: %v", got)
	}
	tt.Move(0, 0)
	if got := ink(); !got.Empty() {
		t.Fatalf("the folded chevron is visible without hover: %v", got)
	}
}
