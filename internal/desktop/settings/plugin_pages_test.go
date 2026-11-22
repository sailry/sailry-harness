package settings

import (
	"os"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestContributedSettingsPages(t *testing.T) {
	const (
		aiGroup             = "settings_group_ai"
		plugin      Section = "plugin:sample"
		pluginLabel         = "Sample extension"
		statusLabel         = "Extension unavailable"
		nativeBody          = "Registered native settings"
		pluginBody          = "Registered plugin settings"
	)
	s := NewLive()
	native := []NativePage{{Section: Providers, Group: aiGroup, Icon: icons.Cpu, Order: 100, Render: func(c *ui.Context, state *State) {
		if state != s {
			t.Fatal("native page rendered with a different settings state")
		}
		ui.Text(c, nativeBody)
	}}}
	plugins := []PluginPage{{Section: plugin, Group: aiGroup, Icon: icons.Plug, Order: 120, Label: pluginLabel, Render: func(c *ui.Context) {
		ui.Text(c, pluginBody)
	}}}
	s.SetNativePages(native)
	s.SetPluginPages(plugins)
	s.Open(Providers)

	var catalog []PluginPage
	var title string
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		catalog, title = s.ContributedPages(c), Title(c, s)
		ui.Row(c).Fill().Children(func() {
			ui.Column(c.Key("nav")).Width(220).FillHeight().Children(func() { Nav(c, s) })
			ui.Column(c.Key("page")).Grow(1).FillHeight().Children(func() { Page(c, s, nil, Look{}) })
		})
	}, 1050, 800)
	for _, lang := range []string{"en", "zh-CN"} {
		tester.SetLocale(lang)
		tester.Frame()
		label := i18n.In(lang, string(Providers))
		description := i18n.In(lang, string(Providers)+"_description")
		if title != label || !tester.HasText(description) || !tester.HasText(nativeBody) {
			t.Fatalf("%s native page has title %q and texts %q", lang, title, tester.Texts())
		}
		if len(catalog) != 2 || !slices.ContainsFunc(catalog, func(page PluginPage) bool {
			return page.Section == Providers && page.Label == label && page.Icon == icons.Cpu && page.Order == 100 && page.Group == aiGroup
		}) || !slices.ContainsFunc(catalog, func(page PluginPage) bool {
			return page.Section == plugin && page.Label == pluginLabel
		}) {
			t.Fatalf("%s navigation and search entries = %+v", lang, catalog)
		}
		nativeButton := findButton(t, tester, label)
		pluginButton := findButton(t, tester, pluginLabel)
		builtInButton := findButton(t, tester, i18n.In(lang, string(Plugins)))
		if nativeButton.Bounds.Y >= pluginButton.Bounds.Y || pluginButton.Bounds.Y >= builtInButton.Bounds.Y {
			t.Fatalf("native, plugin, built-in entries out of order: %v, %v, %v", nativeButton.Bounds, pluginButton.Bounds, builtInButton.Bounds)
		}
	}
	if pages := s.PluginPages(); len(pages) != 1 || pages[0].Section != plugin {
		t.Fatalf("plugin-only catalog = %+v", pages)
	}

	s.SetPluginPages(nil)
	tester.Frame()
	if s.Section != Providers || !s.HasSection(Providers) || s.HasSection(plugin) || len(catalog) != 1 || !tester.HasText(nativeBody) {
		t.Fatal("plugin removal affected the open native page")
	}
	s.SetPluginPages(plugins)
	tester.Frame()
	clickButton(t, tester, pluginLabel)
	if s.Section != plugin || title != pluginLabel || !tester.HasText(pluginBody) {
		t.Fatal("plugin navigation did not open the plugin page")
	}
	s.SetNativePages(nil)
	tester.Frame()
	if s.Section != plugin || !s.HasSection(plugin) || s.HasSection(Providers) || len(catalog) != 1 || !tester.HasText(pluginBody) {
		t.Fatal("native removal affected the open plugin page")
	}

	plugins[0].Label, plugins[0].ContentOnly = statusLabel, true
	s.SetPluginPages(plugins)
	tester.Frame()
	if s.Section != plugin || !s.HasSection(plugin) || title != statusLabel || !tester.HasText(pluginBody) {
		t.Fatal("content-only plugin page did not keep its open route")
	}
	if _, ok := tester.AccessByRole(ui.RoleButton, statusLabel); ok || len(catalog) != 0 || len(s.PluginPages()) != 0 {
		t.Fatal("content-only plugin page appeared in navigation or search")
	}
	s.SetPluginPages(nil)
	if s.Section != Plugins || s.HasSection(plugin) {
		t.Fatalf("removed plugin page fell back to %q", s.Section)
	}
}

func TestMissingSettingsSectionFallsBack(t *testing.T) {
	s := NewLive()
	for _, section := range []Section{Providers, Teams, Connectors, Skills, Hooks, Connections, Terminal, Runtime, Media, Decision, OCR, Knowledge, "unknown", ""} {
		if s.HasSection(section) {
			t.Errorf("unregistered section %q was available", section)
		}
		s.Open(section)
		if s.Section != General {
			t.Errorf("opening %q fell back to %q", section, s.Section)
		}
	}
	s.SetNativePages([]NativePage{{Section: Teams, Group: "settings_group_ai"}})
	s.Open(Teams)
	if s.Section != Teams {
		t.Fatalf("opening a registered section fell back to %q", s.Section)
	}
	s.SetNativePages(nil)
	if s.Section != General || s.HasSection(Teams) {
		t.Fatalf("removed native page fell back to %q", s.Section)
	}

	s.Section = Media
	var title string
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		title = Title(c, s)
		Page(c, s, nil, Look{})
	}, 800, 600)
	tester.SetLocale("en")
	tester.Frame()
	if s.Section != General || title != i18n.In("en", string(General)) || !tester.HasText(i18n.In("en", "settings_general_description")) {
		t.Fatalf("stale section rendered as %q with title %q and texts %q", s.Section, title, tester.Texts())
	}
}

func TestNativeSettingsPageLibraryUsesPageScroll(t *testing.T) {
	s := NewLive()
	s.Knowledge = LocalModels{Kind: ModelsEmbedding, Tab: 1, Model: -1}
	s.SetNativePages([]NativePage{{Section: Knowledge, Group: "settings_group_ai", Render: func(c *ui.Context, state *State) {
		ui.Box(c).Height(1400).Shrink(0)
		LocalModelLibrary(c, state.Model, &state.Knowledge, "knowledge_settings", "knowledge_default_model", "", "knowledge_keyword_only")
	}}})
	s.Open(Knowledge)
	loads := 0
	var detail string
	s.ModelActions = &ModelActions{
		LibraryMore:    func() { loads++; s.LibraryMore = false },
		LibraryDetails: func(id string) { detail = id },
	}
	catalog := i18n.New(os.DirFS("../../../modules/knowledge/desktop"))
	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		theme.Apply(c, "", theme.Dark)
		Page(c, s, nil, Look{})
	}, 900, 650)
	tester.SetLocale("en")
	tester.Frame()
	s.Library = []preview.LibraryModel{{ID: "hub/embedding", Name: "Embedding model", Kind: ModelsEmbedding, Resolved: true}}
	s.LibraryLoad, s.LibraryMore = preview.Fetch{Loaded: true}, true
	tester.Frame()
	tester.Frame()
	if loads != 0 || s.pageScroll.MaxY <= 320 {
		t.Fatalf("library requested %d more pages at the top of scroll extent %v", loads, s.pageScroll.MaxY)
	}
	tester.Scroll(450, 300, 0, 4000)
	tester.Frame()
	tester.Frame()
	if loads != 1 || s.pageScroll.Y == 0 {
		t.Fatalf("library requested %d more pages at scroll offset %v", loads, s.pageScroll.Y)
	}
	clickButton(t, tester, i18n.In("en", "knowledge_model_details"))
	tester.Advance(time.Second)
	if detail != s.Library[0].ID {
		t.Fatalf("model details requested %q", detail)
	}
	if _, ok := tester.AccessByRole(ui.RoleTextField, i18n.In("en", "model_library_query_prefix")); !ok {
		t.Fatal("the native page did not show its model library dialog")
	}
}
