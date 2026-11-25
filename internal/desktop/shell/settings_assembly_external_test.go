package shell_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	browser "github.com/sailry/sailry-harness/modules/browser/desktop"
	computer "github.com/sailry/sailry-harness/modules/computer/desktop"
	connections "github.com/sailry/sailry-harness/modules/connections/desktop"
	connectors "github.com/sailry/sailry-harness/modules/connectors/desktop"
	decision "github.com/sailry/sailry-harness/modules/decision/desktop"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	hooks "github.com/sailry/sailry-harness/modules/hooks/desktop"
	knowledge "github.com/sailry/sailry-harness/modules/knowledge/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
	media "github.com/sailry/sailry-harness/modules/media/desktop"
	memory "github.com/sailry/sailry-harness/modules/memory/desktop"
	ocr "github.com/sailry/sailry-harness/modules/ocr/desktop"
	providers "github.com/sailry/sailry-harness/modules/providers/desktop"
	runtime "github.com/sailry/sailry-harness/modules/runtime/desktop"
	skills "github.com/sailry/sailry-harness/modules/skills/desktop"
	teams "github.com/sailry/sailry-harness/modules/teams/desktop"
	websearch "github.com/sailry/sailry-harness/modules/web-search/desktop"
)

func TestNativeSettingsPageAssembly(t *testing.T) {
	sections := []settings.Section{
		settings.Providers, settings.Connectors, settings.Teams, settings.Skills,
		settings.Knowledge, settings.Memory, settings.Media, settings.Decision,
		settings.Search, settings.Browser, settings.Computer,
		settings.Connections, settings.Hooks, settings.Runtime, settings.OCR,
	}
	for _, window := range []struct {
		name string
		open func(shell.Assembly) *shell.App
	}{
		{"preview", func(assembly shell.Assembly) *shell.App {
			return shell.NewWithAssembly(preview.Load(), assembly)
		}},
		{"live", func(assembly shell.Assembly) *shell.App {
			return shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
		}},
	} {
		t.Run(window.name, func(t *testing.T) {
			full := desktop.Harness()
			workspaceOnly := shell.Assemble(files.Register, market.Register)
			settingsOnly := shell.Assemble(providers.Register, connectors.Register, teams.Register, skills.Register,
				knowledge.Module(), memory.Register, media.Register, decision.Register, websearch.Register,
				browser.Register, computer.Register, connections.Register,
				hooks.Register, runtime.Register, ocr.Register)
			for _, variant := range []struct {
				name                string
				assembly            shell.Assembly
				workspace, settings bool
			}{
				{"default", full, true, true},
				{"workspace-only", workspaceOnly, true, false},
				{"settings-only", settingsOnly, false, true},
				{"omitted", shell.Assembly{}, false, false},
			} {
				t.Run(variant.name, func(t *testing.T) {
					app := window.open(variant.assembly)
					app.Navigate(shell.SettingsPage)
					tester := nativePageTester(app)
					generalBtn, foundGeneral := tester.AccessByRole(ui.RoleButton, i18n.In("en", string(settings.General)))
					if !foundGeneral {
						t.Error("built-in General settings are missing")
					}
					navButton := func(label string) bool {
						for _, item := range tester.AccessAll() {
							if item.Role == ui.RoleButton && item.Name == label && item.Bounds.X == generalBtn.Bounds.X {
								return true
							}
						}
						return false
					}
					for _, section := range sections {
						if app.Settings.HasSection(section) != variant.settings {
							t.Errorf("section %q availability, want %t", section, variant.settings)
						}
						label := i18n.In("en", string(section))
						if navButton(label) != variant.settings {
							t.Errorf("section %q navigation, want %t", section, variant.settings)
						}
						app.Settings.Open(section)
						want := settings.General
						if variant.settings {
							want = section
						}
						if got := app.Settings.Section; got != want {
							t.Errorf("opening %q selected %q, want %q", section, got, want)
						}
						tester.Frame()
					}
					for _, page := range full.WorkspacePages {
						if app.NativePageEnabled(page.Page) != variant.workspace {
							t.Errorf("workspace page %q availability, want %t", page.Page, variant.workspace)
						}
					}
				})
			}
		})
	}
}

func TestKnowledgeModuleAssembly(t *testing.T) {
	for _, test := range []struct {
		name      string
		assembly  shell.Assembly
		embedding bool
	}{
		{"default", desktop.Harness(), true},
		{"collections-only", shell.Assemble(knowledge.Module(knowledge.CollectionsOnly)), false},
	} {
		t.Run(test.name, func(t *testing.T) {
			app := shell.NewWithAssembly(preview.Load(), test.assembly)
			app.Settings.Collections = []viewmodel.KnowledgeCollection{{ID: "notes", Name: "Notes", State: viewmodel.KnowledgeReady}}
			app.Navigate(shell.SettingsPage)
			app.Settings.Open(settings.Knowledge)
			tester := nativePageTester(app)
			tester.SetLocale("zh-CN")
			tester.Frame()
			if !tester.HasText(i18n.In("zh-CN", "knowledge_collections")) {
				t.Error("knowledge collections are missing")
			}
			if !tester.HasText("已是最新 · 0 项") {
				t.Errorf("localized collection status is missing: %q", tester.Texts())
			}
			if _, found := tester.AccessByRole(ui.RoleButton, "向量模型"); found != test.embedding {
				t.Errorf("embedding model control availability, want %t", test.embedding)
			}
		})
	}
}
