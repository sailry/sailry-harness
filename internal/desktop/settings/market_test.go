package settings

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestLiveSkillMarketShowsNodeSearch(t *testing.T) {
	for _, width := range []int{640, 1200} {
		s := NewLive()
		var calls []string
		s.PackageActions = &PackageActions{RetryMarket: func() { calls = append(calls, "retry") }, MoreMarket: func() { calls = append(calls, "more") }}
		s.MarketKind, s.MarketQuery, s.MarketLoad = 1, "pdf", preview.Fetch{Loaded: true}
		// The Node matched both on content the cards do not show, and ranked
		// Zeta first.
		s.MarketSkills = []preview.Skill{{ID: "hub/zeta", Name: "Zeta", Description: "Reads documents", Origin: "Hub"}, {ID: "hub/alpha", Name: "Alpha", Description: "Writes reports", Origin: "Hub"}}
		s.MarketMore = true
		tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, width, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		zeta, zetaShown := tt.Find("Zeta")
		alpha, alphaShown := tt.Find("Alpha")
		if !zetaShown || !alphaShown || zeta.Y > alpha.Y || zeta.Y == alpha.Y && zeta.X > alpha.X {
			t.Fatalf("%v: the live search was filtered or reordered again: %q", width, tt.Texts())
		}
		more, retry := i18n.In("en", "market_load_more"), i18n.In("en", "settings_retry")
		if _, exists := tt.AccessByRole(ui.RoleButton, retry); exists {
			t.Fatalf("%v: a search that read every source offers Retry", width)
		}
		if b := findButton(t, tt, more); b.Bounds.X < 0 || int(b.Bounds.X+b.Bounds.W) > width {
			t.Fatalf("%v: Load more lies outside the window: %+v", width, b.Bounds)
		}
		clickButton(t, tt, more)
		s.SetBusy(BusyMarket, true)
		tt.Frame()
		if b := findButton(t, tt, more); !b.Disabled || !b.Busy {
			t.Fatalf("%v: Load more takes presses while the search reads", width)
		}
		s.SetBusy(BusyMarket, false)
		// A source failed: its cards stay with Retry beside Load more.
		s.MarketFailed = true
		tt.Frame()
		if !tt.HasText("Zeta") {
			t.Fatalf("%v: a partial failure hid the cards read", width)
		}
		clickButton(t, tt, retry)
		// Every source failed: the failure, not an empty search, with Retry.
		s.MarketSkills, s.MarketMore = nil, false
		tt.Frame()
		if !tt.HasText(i18n.In("en", "skill_catalog_load_failed")) || tt.HasText(i18n.In("en", "skills_no_matches")) {
			t.Fatalf("%v: a failed search reads as no matches: %q", width, tt.Texts())
		}
		if _, exists := tt.AccessByRole(ui.RoleButton, more); exists {
			t.Fatalf("%v: a failed search offers Load more", width)
		}
		clickButton(t, tt, retry)
		if !slices.Equal(calls, []string{"more", "retry", "retry"}) {
			t.Fatalf("%v: actions ran %q", width, calls)
		}
		s.MarketFailed = false
		tt.Frame()
		if !tt.HasText(i18n.In("en", "skills_no_matches")) {
			t.Fatalf("%v: an empty search does not say so: %q", width, tt.Texts())
		}
		if _, exists := tt.AccessByRole(ui.RoleButton, retry); exists {
			t.Fatalf("%v: an empty search offers Retry", width)
		}
		s.MarketMore = true
		tt.Frame()
		clickButton(t, tt, more)
		if !slices.Equal(calls, []string{"more", "retry", "retry", "more"}) {
			t.Fatalf("%v: an empty page with a continuation did not page: %q", width, calls)
		}
	}
}

func TestPreviewSkillMarketFiltersLocally(t *testing.T) {
	s := New()
	s.MarketKind, s.MarketQuery, s.MarketLoad = 1, "pdf", preview.Fetch{Loaded: true}
	s.MarketSkills = []preview.Skill{{ID: "a", Name: "PDF tools", Origin: "Hub"}, {ID: "b", Name: "Zeta", Description: "Reads documents", Origin: "Hub"}}
	s.MarketMore, s.MarketFailed = true, true
	tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, 900, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if !tt.HasText("PDF tools") || tt.HasText("Zeta") {
		t.Fatalf("the preview's market does not filter its own cards: %q", tt.Texts())
	}
	for _, name := range []string{i18n.In("en", "market_load_more"), i18n.In("en", "settings_retry")} {
		if _, exists := tt.AccessByRole(ui.RoleButton, name); exists {
			t.Fatalf("the preview offers %q", name)
		}
	}
}

func TestSkillMarketSourceSelector(t *testing.T) {
	s := New()
	s.MarketKind, s.MarketSource, s.MarketLoad = 1, 1, preview.Fetch{Loaded: true}
	tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, 640, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if s.MarketSkillSource != SkillMarketAll || !tt.HasText("changelog") || !tt.HasText("pr-describe") {
		t.Fatalf("default skill source does not show both catalogs: %q", tt.Texts())
	}
	label := func(key string) string { return i18n.In("en", key) }
	search, found := tt.Access(label("market_search_skills"))
	if !found {
		t.Fatal("no search field in the skills market")
	}
	for _, want := range []struct {
		key       string
		selection int
		shown     string
		hidden    string
	}{
		{"skill_source_skills", SkillMarketSkillsSh, "changelog", "pr-describe"},
		{"skill_source_hub", SkillMarketSkillhub, "pr-describe", "changelog"},
	} {
		source, ok := tt.AccessByRole(ui.RoleMenuButton, label("market_details_source"))
		if !ok {
			t.Fatal("no source selector in the skills market")
		}
		if search.Bounds.X+search.Bounds.W > source.Bounds.X ||
			search.Bounds.Y+search.Bounds.H <= source.Bounds.Y || int(source.Bounds.X+source.Bounds.W) > 640 {
			t.Fatalf("source selector is not beside the search field: search=%+v source=%+v", search.Bounds, source.Bounds)
		}
		tt.ClickAt(source.Bounds.X+source.Bounds.W/2, source.Bounds.Y+source.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		var names []string
		var choice ui.AccessInfo
		for _, control := range tt.AccessAll() {
			switch control.Role {
			case ui.RoleMenuItem, ui.RoleMenuItemRadio, ui.RoleMenuItemCheckBox:
				names = append(names, control.Name)
				if control.Name == label(want.key) {
					choice = control
				}
			}
		}
		if !slices.Equal(names, []string{label("market_all"), label("skill_source_skills"), label("skill_source_hub")}) {
			t.Fatalf("skill source choices: %q", names)
		}
		tt.ClickAt(choice.Bounds.X+choice.Bounds.W/2, choice.Bounds.Y+choice.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		if s.MarketSkillSource != want.selection || s.MarketSource != 1 || !tt.HasText(want.shown) || tt.HasText(want.hidden) {
			t.Fatalf("source %q did not filter skills independently of plugins: %q", want.key, tt.Texts())
		}
	}
}
