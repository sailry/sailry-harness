package live

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testFeatureUsage covers the window's anonymous feature counters: what a
// frame, a submission and a saved setting send is an identifier of the
// closed list and nothing else.
func testFeatureUsage(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		const planted = "planted secret /Users/someone/private theme-package-name"
		root, source := terminalTabsFixture(t)
		source.respond = func(request protocol.Request) link.Response {
			if request.Command.Kind == "count_feature" {
				return link.Response{Output: protocol.Output{Kind: "feature_counted"}}
			}
			return link.Response{Output: protocol.Output{Kind: "ok"}}
		}
		root.controller.OnDispatch(countSaved(root.controller))
		counted := func() []string {
			t.Helper()
			synctest.Wait()
			var features []string
			for _, command := range source.sent("count_feature") {
				payload, valid := detail[protocol.CommandCountFeaturePayload](command.Data)
				encoded, err := json.Marshal(command)
				if !valid || err != nil || strings.Contains(string(encoded), "planted") || strings.Contains(string(encoded), "/Users/") {
					t.Fatalf("count_feature carries more than an identifier: %s", encoded)
				}
				if _, official := payload.Feature.OfficialPluginPage(); !payload.Feature.Listed() && !official {
					t.Fatalf("feature %q is outside the closed list", payload.Feature)
				}
				features = append(features, string(payload.Feature))
			}
			source.mu.Lock()
			source.commands = nil
			source.mu.Unlock()
			slices.Sort(features)
			return features
		}
		expect := func(what string, want ...string) {
			t.Helper()
			slices.Sort(want)
			if got := counted(); !slices.Equal(got, want) {
				t.Fatalf("%s counted %v, want %v", what, got, want)
			}
		}

		// What a window opens on and restores is the baseline, not a use.
		root.App.Panel.Show(panel.Tab{Kind: panel.Files, ID: planted})
		root.countFrame()
		root.countFrame()
		expect("the first frames")

		root.App.Page, root.App.Settings.Section, root.App.Theme = shell.SettingsPage, settings.Appearance, planted
		root.App.Panel.Show(panel.Tab{Kind: panel.Browser, ID: planted})
		root.App.Panel.Show(panel.Tab{Kind: panel.Browser, ID: planted})
		root.countFrame()
		expect("a frame", "page.settings", "settings.section.appearance", "panel.tab.browser", "theme.change.package")
		root.countFrame()
		expect("an unchanged frame")
		root.App.Settings.Section, root.App.Theme = settings.Section(planted), ""
		root.countFrame()
		expect("a package's settings page", "settings.section.plugin_page", "theme.change.builtin")

		// A package's page is named only when its Node says the application
		// shipped the package.
		shipped := protocol.PluginInfo{Origin: &protocol.PluginOrigin{Kind: "bundled"}}
		shipped.Summary.Name = "browser"
		installed := protocol.PluginInfo{Origin: &protocol.PluginOrigin{Kind: "online"}}
		installed.Summary.Name = "browser"
		unknown := protocol.PluginInfo{}
		unknown.Summary.Name = "browser"
		root.pluginViews = &pluginViewState{navigation: map[string]pluginNavigationEntry{"shipped": {info: shipped}, "installed": {info: installed}, "unknown": {info: unknown}}}
		for id, want := range map[string]protocol.FeatureUse{"shipped": "page.plugin_page.official:browser", "installed": protocol.FeatureUsePagePluginThirdParty,
			"unknown": protocol.FeatureUsePagePluginThirdParty, "missing": protocol.FeatureUsePagePluginThirdParty} {
			if got := root.pluginPageFeature(id); got != want {
				t.Errorf("package page %s is counted as %q, want %q", id, got, want)
			}
		}

		// A package's views carry the icons and tint it declares: a rail
		// feature its navigation's outline glyph in the text color, a card
		// the name its tile is drawn from. Only an unknown or missing
		// declaration is the puzzle piece.
		root.pluginViews.icons, root.pluginViews.iconFailures = map[string]*ui.SVG{}, map[string]bool{}
		calendar, tint, category := "phosphor:calendar-check", protocol.PluginTintAmber, protocol.PluginCategory("automation")
		outline := "reicon:time/calendar"
		glyph, err := icons.Plugin(outline)
		if err != nil || glyph == icons.Puzzle {
			t.Fatal(err)
		}
		if got := root.pluginNavigationIcon(nil, &protocol.PluginDesktopIcon{Kind: "Name", Data: &outline}); got != glyph {
			t.Error("a navigation entry does not draw its declared glyph")
		}
		for _, missing := range []string{"phosphor:missing", "reicon:missing/icon", "ailogo:missing-brand"} {
			if root.pluginNavigationIcon(nil, &protocol.PluginDesktopIcon{Kind: "Name", Data: &missing}) != icons.Puzzle {
				t.Errorf("the unknown navigation icon %q is not the placeholder", missing)
			}
		}
		if root.pluginNavigationIcon(nil, nil) != icons.Puzzle {
			t.Error("a navigation entry without an icon is not the placeholder")
		}
		// A tile's name in a navigation entry still draws: a Phosphor icon
		// solid, a brand's mark in one color.
		for _, kept := range []string{"reicon:map/globe", calendar, "ailogo:github"} {
			glyph, err := icons.Plugin(kept)
			if err != nil || glyph == icons.Puzzle || root.pluginNavigationIcon(nil, &protocol.PluginDesktopIcon{Kind: "Name", Data: &kept}) != glyph {
				t.Errorf("a navigation entry does not draw %q: %v", kept, err)
			}
		}
		brand := "ailogo:github"
		logo := &protocol.PluginDesktopNavigation{Label: "GitHub", Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: &brand}}
		if view := catalogPackageView(protocol.PluginCatalogEntry{ID: "github", Display: logo}, "en", 0); view.Icon != brand || view.Tint != "" {
			t.Errorf("a brand package's view carries %q %q", view.Icon, view.Tint)
		}
		display := &protocol.PluginDesktopNavigation{Label: "Scheduled tasks", Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: &calendar}}
		declared := protocol.PluginInfo{Extension: &protocol.PluginExtension{Display: display, Tint: &tint, Category: &category}}
		declared.Summary.Name = "scheduled-tasks"
		if view := packageView(declared, "en"); view.Icon != calendar || view.Tint != "amber" || view.Name != "Scheduled tasks" {
			t.Errorf("an installed package's view lost its icon or tint: %q %q", view.Icon, view.Tint)
		}
		if view := catalogPackageView(protocol.PluginCatalogEntry{ID: "scheduled-tasks", Display: display, Tint: &tint}, "en", 0); view.Icon != calendar || view.Tint != "amber" {
			t.Errorf("a catalog entry's view lost its icon or tint: %q %q", view.Icon, view.Tint)
		}
		own := protocol.PluginDesktopIconSvgPayload{Svg: `<svg viewBox="0 0 24 24"><path d="M4 12h16"/></svg>`}
		if got := pluginIconSource(&protocol.PluginDesktopIcon{Kind: "Svg", Data: &own}); got != own.Svg || pluginIconSource(nil) != "" {
			t.Errorf("a package's own SVG is carried as %q", got)
		}
		if view := packageView(unknown, "en"); view.Icon != "" || view.Tint != "" {
			t.Error("a package declaring no icon has one")
		}
		// An update names the permissions it asks beyond the installed ones.
		had := &protocol.PluginExtension{Actions: []protocol.PluginAction{protocol.PluginActionReadArtifacts}}
		asked := []protocol.PluginAction{protocol.PluginActionReadArtifacts, protocol.PluginActionWriteArtifacts}
		if added := addedPermissions(had, asked); len(added) != 1 || added[0] != "market_permission_artifacts_write" {
			t.Errorf("added permissions = %q", added)
		}
		if added := addedPermissions(nil, asked); len(added) != 2 {
			t.Errorf("a package without an extension gains %q", added)
		}

		actions := &conversationActions{r: root, cv: &viewmodel.Conversation{ID: planted}, comp: composer.New("")}
		actions.countDictation()
		actions.countSubmit(composer.Message{Text: planted, Attachments: []*composer.Attachment{{}}, Model: planted, Mode: composer.Plan, Permission: composer.FullAccess}, false, true)
		actions.countSubmit(composer.Message{Text: planted, References: []composer.Reference{{}}}, true, false)
		expect("two submissions", "dictation.start", "composer.submit", "composer.submit", "composer.submit.has_attachment", "composer.submit.dictated", "composer.submit.queued",
			"composer.submit.has_reference", "composer.submit.guided", "composer.mode.plan", "composer.mode.code", "composer.permission.full", "composer.permission.ask")
		root.CountReadAloud(true)
		root.CountReadAloud(false)
		expect("two readings", "read_aloud.model_voice", "read_aloud.os_voice")

		// A saved setting is counted by its family, from the command's kind.
		if _, err := root.controller.Do(root.ctx, protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{}}); err != nil {
			t.Fatal(err)
		}
		if _, err := root.controller.Do(root.ctx, protocol.Command{Kind: "read_power_settings"}); err != nil {
			t.Fatal(err)
		}
		expect("a saved setting", "settings.save.power")
		for kind, feature := range savedFeatures {
			if !slices.Contains(protocol.CommandKinds(), kind) || !feature.Listed() {
				t.Errorf("saved setting %q -> %q is not a command or not a listed feature", kind, feature)
			}
		}
		for _, features := range []map[string]protocol.FeatureUse{flatten(pageFeatures), flatten(sectionFeatures), flatten(panelFeatures), flatten(modeFeatures), flatten(permissionFeatures)} {
			for from, feature := range features {
				if !feature.Listed() {
					t.Errorf("%s -> %q is not a listed feature", from, feature)
				}
			}
		}

		// The preview counts nothing.
		root.App.Preview = true
		root.App.Page = shell.FilesPage
		root.countFrame()
		actions.countSubmit(composer.Message{}, false, false)
		expect("the preview")
	})
}

func flatten[K comparable](features map[K]protocol.FeatureUse) map[string]protocol.FeatureUse {
	out := map[string]protocol.FeatureUse{}
	for _, feature := range features {
		out[string(feature)] = feature
	}
	return out
}
