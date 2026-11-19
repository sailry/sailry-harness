package settings

import (
	"fmt"
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
		s.PackageActions = &PackageActions{RetryMarket: func() { calls = append(calls, "retry") }}
		s.MarketKind, s.MarketQuery, s.MarketLoad = 1, "pdf", preview.Fetch{Loaded: true}
		// The Node matched both on content the cards do not show, and ranked
		// Zeta first.
		s.MarketSkills = []preview.Skill{{ID: "hub/zeta", Name: "Zeta", Description: "Reads documents", Origin: "Hub"}, {ID: "hub/alpha", Name: "Alpha", Description: "Writes reports", Origin: "Hub"}}
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
		// A source failed: its cards stay with Retry.
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
		if !slices.Equal(calls, []string{"retry", "retry"}) {
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
	}
}

func TestSkillMarketPagesAtScrollEnd(t *testing.T) {
	s := NewLive()
	s.MarketKind, s.MarketMore, s.MarketLoad = 1, true, preview.Fetch{Loaded: true}
	for i := range 26 {
		id := fmt.Sprintf("Skill %02d", i)
		s.MarketSkills = append(s.MarketSkills, preview.Skill{ID: id, Name: id})
	}
	calls := 0
	s.PackageActions = &PackageActions{MoreMarket: func() {
		calls++
		s.MarketPaging, s.MarketPagingFailed = true, false
		s.SetBusy(BusyMarket, true)
	}}
	tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Advance(time.Second)
	if calls != 0 || s.marketScroll.MaxY <= 320 {
		t.Fatalf("near-end load before scrolling: %d requests, scroll %+v", calls, s.marketScroll)
	}
	more := i18n.In("en", "market_load_more")
	if _, exists := tt.AccessByRole(ui.RoleButton, more); exists {
		t.Fatal("skill market still has a Load more button")
	}
	tt.Scroll(450, 400, 0, 10_000)
	tt.Advance(time.Second)
	if calls != 1 {
		t.Fatalf("scrolling to the bottom requested %d pages", calls)
	}
	mark := i18n.In("en", "activity_running")
	if info, ok := tt.AccessByRole(ui.RoleProgress, mark); !ok || info.Offscreen || info.Bounds.W != 16 || info.Bounds.H != 16 {
		t.Fatalf("no visible list loader: %+v", info)
	}
	s.MarketPaging, s.MarketPagingFailed = false, true
	s.SetBusy(BusyMarket, false)
	tt.Frame()
	tt.Advance(time.Second)
	if calls != 1 || !tt.HasText("Skill 25") {
		t.Fatalf("failed page retried automatically or hid existing cards: %d requests", calls)
	}
	// The shorter idle footer may leave the retry below the current view.
	tt.Scroll(450, 400, 0, 10_000)
	clickButton(t, tt, i18n.In("en", "settings_retry"))
	if calls != 2 {
		t.Fatalf("page retry left %d requests", calls)
	}
	s.MarketPaging, s.MarketMore = false, false
	s.SetBusy(BusyMarket, false)
	s.MarketSkills = append(s.MarketSkills, preview.Skill{ID: "last", Name: "Last skill"})
	tt.Frame()
	tt.Advance(time.Second)
	if _, ok := tt.AccessByRole(ui.RoleProgress, mark); ok || calls != 2 {
		t.Fatalf("finished list is still loading, %d requests", calls)
	}
}

func TestSkillMarketContinuesAnEmptyPage(t *testing.T) {
	s := NewLive()
	s.MarketKind, s.MarketMore, s.MarketLoad = 1, true, preview.Fetch{Loaded: true}
	s.PackageActions = &PackageActions{MoreMarket: func() {
		s.MarketPaging = true
		s.SetBusy(BusyMarket, true)
	}}
	tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	if tt.HasText(i18n.In("en", "skills_no_matches")) {
		t.Fatal("the market reported an empty search before the last page")
	}
	if _, ok := tt.AccessByRole(ui.RoleProgress, i18n.In("en", "activity_running")); !ok || !s.MarketPaging {
		t.Fatal("an empty page with a continuation did not start loading")
	}
}

func TestThemeMarketHasNoHostLabel(t *testing.T) {
	s := New()
	s.MarketKind, s.MarketLoad = 2, preview.Fetch{Loaded: true}
	tt := ui.NewTesterWith(func(c *ui.Context) { MarketPage(c, s) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if !tt.HasText(i18n.In("en", "market_search_themes")) || tt.HasText(i18n.In("en", "market_local")) {
		t.Fatalf("unexpected theme market controls: %q", tt.Texts())
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
