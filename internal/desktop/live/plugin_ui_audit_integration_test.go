//go:build integration

package live

import (
	"cmp"
	"context"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// pluginAuditPackages are the bundled packages that contribute a rail page or
// a Settings page. Files and Git stay native core pages.
var pluginAuditPackages = []string{
	"activity-board", "browser", "city-trader", "databases", "doudizhu", "external-browser", "gomoku",
	"liars-dice", "memory", "poker", "reminders", "reversi", "scheduled-tasks", "ssh",
	"statistics", "task-plaza", "web-search", "xiangqi",
}

// auditPluginPages renders every bundled plugin page from its real manifest
// in Chinese and English, light and dark. It is a step of
// TestPluginUIIntegration; SAILRY_UI_SHOTS names a directory that receives one
// frame per page.
func auditPluginPages(t *testing.T, runtime *Runtime, do func(context.Context, protocol.Command) protocol.Output) {
	ctx, cancel := context.WithTimeout(t.Context(), 8*time.Minute)
	defer cancel()
	shots := os.Getenv("SAILRY_UI_SHOTS")
	if shots != "" {
		if err := os.MkdirAll(shots, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range pluginAuditPackages {
		if !slices.ContainsFunc(runtime.controller.Snapshot().Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == name }) {
			do(ctx, protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: name}})
		}
		output := do(ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}})
		info, exists := detail[protocol.PluginInfo](output.Data)
		if !exists {
			t.Fatalf("bundled package %q was not returned", name)
		}
		if !info.Summary.Enabled {
			do(ctx, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
				Name: name, ExpectedRevision: info.Summary.Revision, Enabled: true,
			}})
		}
	}
	runtime.App.NavOpen, runtime.App.PanelOpen = true, false
	tester := ui.NewTesterWith(runtime.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	settle := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			if ready() {
				return
			}
			if ctx.Err() != nil || time.Now().After(deadline) {
				t.Fatalf("%s did not settle; visible text: %q", label, tester.Texts())
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	// Package reads, callbacks and their Node commands complete off the UI
	// thread: frames keep pumping until the view stops changing.
	rest := func() {
		for range 40 {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			time.Sleep(10 * time.Millisecond)
		}
		tester.Advance(time.Second)
		tester.Frame()
	}
	save := func(name string) {
		if shots == "" {
			return
		}
		file, err := os.Create(filepath.Join(shots, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	}
	settle("audited inventory", func() bool {
		if runtime.pluginViews == nil {
			return false
		}
		for _, name := range pluginAuditPackages {
			info, exists := runtime.packages.infos[name]
			if !exists || info.Extension == nil || !info.Summary.Enabled {
				return false
			}
		}
		return true
	})
	for _, locale := range []string{"zh-CN", "en"} {
		for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
			suffix := locale + "-dark"
			if mode == theme.Light {
				suffix = locale + "-light"
			}
			tester.SetLocale(locale)
			tester.SetDark(mode == theme.Dark)
			runtime.App.Mode = mode
			settle("package locale "+locale, func() bool { return runtime.packageLocale() == locale })
			tester.Frame()
			entries := make([]pluginNavigationEntry, 0, len(runtime.pluginViews.navigation))
			for _, entry := range runtime.pluginViews.navigation {
				if entry.scope.Surface.Kind != "settings" {
					entries = append(entries, entry)
				}
			}
			slices.SortFunc(entries, func(first, second pluginNavigationEntry) int { return cmp.Compare(first.id, second.id) })
			for _, entry := range entries {
				name := entry.info.Summary.Name
				label := pluginNavigation(*entry.info.Extension.Desktop.Navigation, locale)
				order, pins := runtime.App.FeaturePreferences()
				for id := range runtime.pluginViews.navigation {
					pins[id] = id == entry.id
				}
				runtime.App.SetFeaturePreferences(order, pins)
				tester.Frame()
				if err := tester.Click(label); err != nil {
					t.Errorf("%s %s: rail entry %q: %v", name, suffix, label, err)
					continue
				}
				key := pluginMountKey{scope: pluginScopeIdentity(entry.owner.controller.Target(), entry.scope), role: "navigation", name: name}
				settle(name+" page", func() bool {
					mounted := runtime.pluginViews.mounts[key]
					return runtime.App.Page == shell.PluginPage && mounted != nil && (mounted.mounted != nil || mounted.failure != nil)
				})
				rest()
				if mounted := runtime.pluginViews.mounts[key]; mounted == nil || mounted.failure != nil {
					t.Errorf("%s %s: page failed: %v", name, suffix, mounted.failure)
				}
				save(fmt.Sprintf("%s-page-%s", name, suffix))
				if name == "scheduled-tasks" {
					auditScheduledTasks(t, tester, locale, rest, func(step string) { save(fmt.Sprintf("%s-%s-%s", name, step, suffix)) })
				}
			}
			sections := make([]settings.Section, 0, len(runtime.pluginViews.settings))
			for section := range runtime.pluginViews.settings {
				sections = append(sections, section)
			}
			slices.Sort(sections)
			for _, section := range sections {
				name := runtime.pluginViews.settings[section].info.Summary.Name
				runtime.App.Settings.Open(section)
				runtime.App.Navigate(shell.SettingsPage)
				rest()
				save(fmt.Sprintf("%s-settings-%s", name, suffix))
			}
			runtime.App.Navigate(shell.ConversationPage)
			tester.Frame()
		}
	}
	for _, mounted := range runtime.pluginViews.mounts {
		if mounted.failure != nil {
			t.Errorf("package %s failed: %v", mounted.scope.Package.Name, mounted.failure)
		}
	}
}

// auditScheduledTasks asserts the scheduled-tasks page uses the Kit select,
// the Kit empty state and one content column, in the package's own locale.
func auditScheduledTasks(t *testing.T, tester *ui.Tester, locale string, rest func(), save func(string)) {
	t.Helper()
	text := map[string][5]string{
		"en":    {"Scheduled tasks", "Run tasks on a schedule", "All projects", "New task", "No tasks"},
		"zh-CN": {"定时任务", "按计划运行任务", "全部项目", "新建任务", "暂无任务"},
	}[locale]
	for _, value := range text {
		if !tester.HasText(value) {
			t.Fatalf("scheduled tasks %s: %q is not shown; visible text: %q", locale, value, tester.Texts())
		}
	}
	// One content column: the description, the select and the empty card
	// share a left edge, and the primary action ends at its right edge.
	description, _ := tester.Find(text[1])
	field, found := tester.Access(map[string]string{"en": "Project", "zh-CN": "项目"}[locale])
	if !found || !tester.HasText(text[2]) {
		t.Fatalf("scheduled tasks %s: the project select is missing; visible text: %q", locale, tester.Texts())
	}
	create, exists := tester.Access(text[3])
	if !exists || field.Bounds.X != description.X || field.Bounds.W != 192 || field.Bounds.H < 30 || create.Bounds.X+create.Bounds.W != description.X+kitPageColumn {
		t.Fatalf("scheduled tasks %s: column %v, select %v, action %v", locale, description, field.Bounds, create.Bounds)
	}
	// Kit's empty state, centered in the column under its round tile.
	empty, _ := tester.Find(text[4])
	if center := empty.X + empty.W/2; center < description.X+kitPageColumn/2-1 || center > description.X+kitPageColumn/2+1 ||
		empty.Y-field.Bounds.Y-field.Bounds.H < 24+40+48+12 {
		t.Fatalf("scheduled tasks %s: empty state %v is not centered under select %v", locale, empty, field.Bounds)
	}
	// The Kit select opens Kit's menu at its own width.
	tester.ClickAt(field.Bounds.X+10, field.Bounds.Y+5)
	tester.Frame()
	tester.Advance(200 * time.Millisecond)
	tester.Frame()
	menu, exists := tester.AccessByRole(ui.RoleMenu, "")
	if !exists || menu.Bounds.W != field.Bounds.W {
		t.Fatalf("scheduled tasks %s: select menu %v, select %v", locale, menu.Bounds, field.Bounds)
	}
	save("select")
	tester.Key(0, ui.KeyEscape)
	rest()
	if _, open := tester.AccessByRole(ui.RoleMenu, ""); open {
		t.Fatalf("scheduled tasks %s: select menu stayed open", locale)
	}
	if err := tester.Click(text[3]); err != nil {
		t.Fatal(err)
	}
	rest()
	save("dialog")
	tester.Key(0, ui.KeyEscape)
	rest()
}

// kitPageColumn is the content width of kit.Page: its 800 column less its
// padding of 24 a side.
const kitPageColumn = 752
