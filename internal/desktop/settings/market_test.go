package settings

import (
	"slices"
	"testing"

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
