package live

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
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
