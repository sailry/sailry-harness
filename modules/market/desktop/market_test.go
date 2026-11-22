package desktop

import (
	"fmt"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestMarketPageUsesSelectedNode(t *testing.T) {
	s := settings.NewLive()
	s.ProviderActions.Hosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{
			{Node: "local", Local: true, Connected: true},
			{Node: "remote", Name: "Build Host", Connected: true},
		}
	}
	s.MarketLoad.Loaded = true
	var installs, details []string
	s.PackageActions = &settings.PackageActions{
		MarketPlugin: func(id string) { installs = append(installs, s.ManagedNode+"/"+id) },
		LoadDetails: func(kind settings.DetailKind, id string, done func(string, string)) {
			if kind != settings.DetailPlugin {
				t.Fatalf("unexpected details kind %v", kind)
			}
			details = append(details, s.ManagedNode+"/"+id)
			done("", "")
		},
	}
	app, page := &shell.App{Settings: s}, Page()
	tt := ui.NewTesterWith(func(c *ui.Context) {
		id, name := "local-plugin", "Local Plugin"
		if s.ManagedNode == "remote" {
			id, name = "remote-plugin", "Remote Plugin"
		}
		s.MarketPlugins = []preview.Package{{ID: id, Name: name, Source: "market_details_source_official", Installable: true}}
		page.Render(c, app)
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	clickButton(t, tt, "Local Plugin")
	if s.Details == nil || s.Details.ID != "local-plugin" {
		t.Fatal("the local plugin did not open its details")
	}
	host, found := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "project_host"))
	if !found {
		t.Fatal("the plugin market has no Host selector")
	}
	tt.ClickAt(host.Bounds.X+host.Bounds.W/2, host.Bounds.Y+host.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	choice, found := tt.Access("Build Host")
	if !found {
		t.Fatal("the selected Node menu has no remote choice")
	}
	tt.ClickAt(choice.Bounds.X+choice.Bounds.W/2, choice.Bounds.Y+choice.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if s.ManagedNode != "remote" || s.Details != nil || tt.HasText("Local Plugin") {
		t.Fatalf("the Host change retained the old scope: node=%q details=%v text=%q", s.ManagedNode, s.Details, tt.Texts())
	}
	clickButton(t, tt, "Remote Plugin")
	clickButton(t, tt, i18n.In("en", "market_install"))
	if !slices.Equal(details, []string{"/local-plugin", "remote/remote-plugin"}) || !slices.Equal(installs, []string{"remote/remote-plugin"}) {
		t.Fatalf("market actions reached the wrong scope: details=%q installs=%q", details, installs)
	}
}

func TestMarketPageKeepsRendererStatePerWindow(t *testing.T) {
	page := Page()
	first, second := settings.NewLive(), settings.NewLive()
	first.MarketLoad.Loaded, second.MarketLoad.Loaded = true, true
	second.MarketKind = 1
	window := func(s *settings.State) *ui.Tester {
		app := &shell.App{Settings: s}
		return ui.NewTesterWith(func(c *ui.Context) { page.Render(c, app) }, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	}
	a, b := window(first), window(second)
	a.Frame()
	first.ShowDetails(settings.DetailPlugin, "example")
	b.Frame()
	a.Frame()
	if first.Details == nil || first.Details.ID != "example" {
		t.Fatal("rendering another window's catalog closed the original window's details")
	}
}

func TestLiveSkillMarketShowsNodeSearch(t *testing.T) {
	for _, width := range []int{640, 1200} {
		s := settings.NewLive()
		r := &renderer{}
		var calls []string
		s.PackageActions = &settings.PackageActions{RetryMarket: func() { calls = append(calls, "retry") }}
		s.MarketKind, s.MarketQuery, s.MarketLoad = 1, "pdf", preview.Fetch{Loaded: true}
		// The Node matched both on content the cards do not show, and ranked
		// Zeta first.
		s.MarketSkills = []preview.Skill{{ID: "hub/zeta", Name: "Zeta", Description: "Reads documents", Origin: "Hub"}, {ID: "hub/alpha", Name: "Alpha", Description: "Writes reports", Origin: "Hub"}}
		tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, width, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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
	s := settings.NewLive()
	r := &renderer{}
	s.MarketKind, s.MarketMore, s.MarketLoad = 1, true, preview.Fetch{Loaded: true}
	for i := range 26 {
		id := fmt.Sprintf("Skill %02d", i)
		s.MarketSkills = append(s.MarketSkills, preview.Skill{ID: id, Name: id})
	}
	calls := 0
	s.PackageActions = &settings.PackageActions{MoreMarket: func() {
		calls++
		s.MarketPaging, s.MarketPagingFailed = true, false
		s.SetBusy(settings.BusyMarket, true)
	}}
	tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Advance(time.Second)
	if calls != 0 || r.scroll.MaxY <= 320 {
		t.Fatalf("near-end load before scrolling: %d requests, scroll %+v", calls, r.scroll)
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
	s.SetBusy(settings.BusyMarket, false)
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
	s.SetBusy(settings.BusyMarket, false)
	s.MarketSkills = append(s.MarketSkills, preview.Skill{ID: "last", Name: "Last skill"})
	tt.Frame()
	tt.Advance(time.Second)
	if _, ok := tt.AccessByRole(ui.RoleProgress, mark); ok || calls != 2 {
		t.Fatalf("finished list is still loading, %d requests", calls)
	}
}

func TestSkillMarketContinuesAnEmptyPage(t *testing.T) {
	s := settings.NewLive()
	r := &renderer{}
	s.MarketKind, s.MarketMore, s.MarketLoad = 1, true, preview.Fetch{Loaded: true}
	s.PackageActions = &settings.PackageActions{MoreMarket: func() {
		s.MarketPaging = true
		s.SetBusy(settings.BusyMarket, true)
	}}
	tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
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
	s := settings.NewLive()
	s.ProviderActions.Hosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{
			{Node: "local", Local: true, Connected: true},
			{Node: "remote", Name: "Build Host"},
		}
	}
	s.ManagedNode = "remote"
	s.Themes = []preview.MarketTheme{{ID: "example", Name: "Example Theme", Light: true}}
	r := &renderer{}
	s.MarketKind, s.MarketLoad = 2, preview.Fetch{Loaded: true}
	tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	_, hasSelector := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "project_host"))
	if !tt.HasText(i18n.In("en", "market_search_themes")) || !tt.HasText("Example Theme") || hasSelector || tt.HasText(i18n.In("en", "market_local")) {
		t.Fatalf("unexpected theme market controls: %q", tt.Texts())
	}
}

func TestPreviewSkillMarketFiltersLocally(t *testing.T) {
	s := settings.New()
	r := &renderer{}
	s.MarketKind, s.MarketQuery, s.MarketLoad = 1, "pdf", preview.Fetch{Loaded: true}
	s.MarketSkills = []preview.Skill{{ID: "a", Name: "PDF tools", Origin: "Hub"}, {ID: "b", Name: "Zeta", Description: "Reads documents", Origin: "Hub"}}
	s.MarketMore, s.MarketFailed = true, true
	tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, 900, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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
	s := settings.New()
	r := &renderer{}
	s.MarketKind, s.MarketSource, s.MarketLoad = 1, 1, preview.Fetch{Loaded: true}
	tt := ui.NewTesterWith(func(c *ui.Context) { r.render(c, s.MarketScope()) }, 640, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if s.MarketSkillSource != settings.SkillMarketAll || !tt.HasText("changelog") || !tt.HasText("pr-describe") {
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
		{"skill_source_skills", settings.SkillMarketSkillsSh, "changelog", "pr-describe"},
		{"skill_source_hub", settings.SkillMarketSkillhub, "pr-describe", "changelog"},
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

func TestCompactCount(t *testing.T) {
	for _, test := range []struct {
		locale string
		counts map[uint64]string
	}{
		{"en", map[uint64]string{0: "0", 950: "950", 1_234: "1.2K", 12_345: "12.3K", 999_960: "1M", 4_400_000: "4.4M"}},
		{"zh-CN", map[uint64]string{950: "950", 12_345: "1.2万", 4_400_000: "440万", 340_000_000: "3.4亿"}},
		{"zh-TW", map[uint64]string{12_345: "1.2萬", 340_000_000: "3.4億"}},
		{"ja", map[uint64]string{12_345: "1.2万", 340_000_000: "3.4億"}},
	} {
		for n, want := range test.counts {
			if got := compactCount(test.locale, n); got != want {
				t.Errorf("compactCount(%q, %d) = %q; want %q", test.locale, n, got, want)
			}
		}
	}
}

func clickButton(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	b, found := tt.AccessByRole(ui.RoleButton, name)
	if !found || b.Disabled {
		t.Fatalf("no enabled %q button: %q", name, tt.Texts())
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
}
