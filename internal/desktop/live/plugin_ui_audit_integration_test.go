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
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testplugin"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var pluginAuditPackages = []string{testplugin.DesktopName}

// auditPluginPages renders the local package's navigation and Settings entries
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
		output := do(ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}})
		info, exists := detail[protocol.PluginInfo](output.Data)
		if !exists {
			t.Fatalf("local fixture %q was not returned", name)
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
				if entry.scope.Surface.Kind != "settings" && slices.Contains(pluginAuditPackages, entry.info.Summary.Name) {
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
				if mounted := runtime.pluginViews.mounts[key]; mounted == nil {
					t.Fatalf("%s %s: page was not mounted", name, suffix)
				} else if mounted.failure != nil {
					t.Fatalf("%s %s: page failed: %v", name, suffix, mounted.failure)
				}
				save(fmt.Sprintf("%s-page-%s", name, suffix))
				auditFixturePage(t, tester, locale)
				auditFixturePageControls(t, tester, locale, settle, func(step string) { save(fmt.Sprintf("%s-%s-%s", name, step, suffix)) })
			}
			if len(entries) != 1 || entries[0].info.Summary.Name != testplugin.DesktopName {
				t.Fatalf("expected the local navigation fixture, got %+v", entries)
			}
			sections := make([]settings.Section, 0, len(runtime.pluginViews.settings))
			for section := range runtime.pluginViews.settings {
				sections = append(sections, section)
			}
			if len(sections) != 1 {
				t.Fatalf("expected the local Settings fixture, got %v", sections)
			}
			slices.Sort(sections)
			for _, section := range sections {
				name := runtime.pluginViews.settings[section].info.Summary.Name
				if name != testplugin.DesktopName {
					t.Fatalf("Settings section %q belongs to %q, not the fixture", section, name)
				}
				runtime.App.Settings.Open(section)
				runtime.App.Navigate(shell.SettingsPage)
				rest()
				auditFixturePage(t, tester, locale)
				save(fmt.Sprintf("%s-settings-%s", name, suffix))
			}
			runtime.App.Navigate(shell.ConversationPage)
			tester.Frame()
		}
	}
	// The native Search settings page is exercised last, in Chinese and dark: its
	// drags and saves come after every page was drawn at rest.
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	runtime.App.Mode = theme.Dark
	settle("package locale zh-CN", func() bool { return runtime.packageLocale() == "zh-CN" })
	runtime.App.Settings.Open(settings.Search)
	runtime.App.Navigate(shell.SettingsPage)
	rest()
	auditSearchSettings(t, ctx, runtime, tester, do, settle, rest, func(step string) { save("web-search-" + step + "-zh-CN-dark") })
	for _, mounted := range runtime.pluginViews.mounts {
		if mounted.failure != nil {
			t.Errorf("package %s failed: %v", mounted.scope.Package.Name, mounted.failure)
		}
	}
}

func auditFixturePage(t *testing.T, tester *ui.Tester, locale string) {
	t.Helper()
	labels := map[string][3]string{
		"en":    {"Desktop fixture", "Fixture ready", "Increment"},
		"zh-CN": {"桌面夹具", "已就绪", "递增"},
	}[locale]
	for _, label := range labels {
		if !tester.HasText(label) {
			t.Fatalf("fixture %s: %q is not shown: %q", locale, label, tester.Texts())
		}
	}
	if control, found := tester.AccessByRole(ui.RoleButton, labels[2]); !found || control.Disabled || control.Bounds.W <= 0 || control.Bounds.H <= 0 {
		t.Fatalf("fixture %s: no enabled retained callback: %+v", locale, control)
	}
}

func auditFixturePageControls(t *testing.T, tester *ui.Tester, locale string, settle func(string, func() bool), save func(string)) {
	t.Helper()
	labels := map[string][7]string{
		"en":    {"Fixture ready", "Fixture project", "All projects", "No fixture tasks", "Open fixture dialog", "Fixture dialog body", "Local project"},
		"zh-CN": {"已就绪", "夹具项目", "全部项目", "暂无夹具任务", "打开夹具对话框", "夹具对话框内容", "本地项目"},
	}[locale]
	description, _ := tester.Find(labels[0])
	field, found := tester.Access(labels[1])
	create, exists := tester.AccessByRole(ui.RoleButton, labels[4])
	const columnWidth = kit.PageWidth - 2*24
	if !found || !exists || field.Bounds.X != description.X || field.Bounds.W != 192 || field.Bounds.H < 30 ||
		create.Bounds.X+create.Bounds.W != description.X+columnWidth || !tester.HasText(labels[2]) {
		t.Fatalf("fixture %s: column %v, select %v, action %v", locale, description, field.Bounds, create.Bounds)
	}
	empty, shown := tester.Find(labels[3])
	if center := empty.X + empty.W/2; !shown || center < description.X+columnWidth/2-1 || center > description.X+columnWidth/2+1 ||
		empty.Y-field.Bounds.Y-field.Bounds.H < 24+40+48+12 {
		t.Fatalf("fixture %s: empty state %v is not centered under select %v", locale, empty, field.Bounds)
	}
	tester.ClickAt(field.Bounds.X+10, field.Bounds.Y+5)
	settle("the fixture SelectField opens the Kit menu", func() bool {
		menu, open := tester.AccessByRole(ui.RoleMenu, "")
		return open && menu.Bounds.W == field.Bounds.W && tester.HasText(labels[6])
	})
	save("select")
	tester.Key(0, ui.KeyEscape)
	settle("Escape closes the fixture menu", func() bool {
		_, open := tester.AccessByRole(ui.RoleMenu, "")
		return !open
	})
	if err := tester.Click(labels[4]); err != nil {
		t.Fatal(err)
	}
	settle("the retained action opens the fixture dialog", func() bool { return tester.HasText(labels[5]) })
	save("dialog")
	tester.Key(0, ui.KeyEscape)
	settle("Escape closes the fixture dialog", func() bool { return !tester.HasText(labels[5]) })
}

// auditSearchSettings asserts the native Search settings page in
// Chinese: its rows keep the application's row metrics, the overflow menu is
// an icon button, service names align after their leading icons, and dragging
// a handle or using the keyboard saves the order on the Node. It is a step of
// TestPluginUIIntegration.
func auditSearchSettings(t *testing.T, ctx context.Context, runtime *Runtime, tester *ui.Tester, do func(context.Context, protocol.Command) protocol.Output,
	settle func(string, func() bool), rest func(), save func(string)) {
	t.Helper()
	find := func(text string) ui.Rect {
		t.Helper()
		box, exists := tester.Find(text)
		if !exists {
			t.Fatalf("search settings: %q is not shown; visible text: %q", text, tester.Texts())
		}
		return box
	}
	named := func(role ui.Role, name string) (found []ui.AccessInfo) {
		for _, info := range tester.AccessAll() {
			if info.Role == role && info.Name == name {
				found = append(found, info)
			}
		}
		return
	}
	order := func() []protocol.SearchDriver {
		t.Helper()
		saved, valid := detail[protocol.SearchSettings](do(ctx, protocol.Command{Kind: "read_search_settings"}).Data)
		if !valid {
			t.Fatal("search settings were not returned")
		}
		return saved.Order
	}
	services := order()
	firstService := slices.Index(services, protocol.SearchDriverExa)
	if firstService < 0 || firstService+2 >= len(services) ||
		services[firstService+1] != protocol.SearchDriverTinyfish || services[firstService+2] != protocol.SearchDriverTavily {
		t.Fatalf("search settings: expected adjacent Exa, TinyFish and Tavily in %v", services)
	}
	// The application's rows: a title alone is the minimum row height, and
	// a described row is its padding, title, gap and description.
	service, fallback := find("默认服务"), find("使用备用服务")
	// The Kit select of the first row is a fraction over its 32 control.
	if pitch := fallback.Y - service.Y; pitch < kit.RowMinHeight || pitch >= kit.RowMinHeight+1 {
		t.Errorf("search settings: rows %v and %v are not %v apart", service, fallback, float32(kit.RowMinHeight))
	}
	// Each described row is as high as its own two lines: a detail mixing
	// scripts is a fraction higher than one in a single script.
	exa, quota, tinyfish, key, tavily := find("Exa"), find("匿名额度"), find("TinyFish"), find("需要 API 密钥 · 免费额度"), find("Tavily")
	near := func(got, want float32) bool { return got > want-0.01 && got < want+0.01 }
	if !near(quota.Y-exa.Y-exa.H, kit.TitleGap) || !near(tinyfish.Y-exa.Y, 2*kit.RowPad+exa.H+kit.TitleGap+quota.H) ||
		!near(key.Y-tinyfish.Y-tinyfish.H, kit.TitleGap) || !near(tavily.Y-tinyfish.Y, 2*kit.RowPad+tinyfish.H+kit.TitleGap+key.H) || exa.H != service.H {
		t.Errorf("search settings: service rows Exa %v, its detail %v, TinyFish %v, its detail %v, Tavily %v are off the row metrics", exa, quota, tinyfish, key, tavily)
	}
	// One handle and one overflow icon button per service, with no arrow
	// buttons or visible text labels on the overflow controls.
	handles, more := named(ui.RoleButton, "拖动排序"), named(ui.RoleMenuButton, "更多")
	shown := len(named(ui.RoleText, "更多"))
	arrows := len(named(ui.RoleButton, "上移")) + len(named(ui.RoleButton, "下移"))
	if len(handles) != len(services) || len(more) != len(services) || shown != 0 || arrows != 0 {
		t.Fatalf("search settings: %d handles, %d overflow menus, %d overflow texts, %d arrows for %d services",
			len(handles), len(more), shown, arrows, len(services))
	}
	for _, button := range more {
		if button.Bounds.W != 28 || button.Bounds.H != 28 {
			t.Errorf("search settings: the overflow menu is %v, want a 28 DIP icon button", button.Bounds)
		}
	}
	for i, title := range []ui.Rect{exa, tinyfish, tavily} {
		handle := handles[firstService+i].Bounds
		if title.X != exa.X || !near(title.X-handle.X-handle.W, 8+kit.RowIcon+kit.RowIconGap) {
			t.Errorf("search settings: title %v is not aligned after the handle %v and leading icon", title, handle)
		}
	}
	// Drag Exa by its handle below Tavily.
	grip, targetGrip := handles[firstService].Bounds, handles[firstService+2].Bounds
	x, y := grip.X+grip.W/2, grip.Y+grip.H/2
	target := targetGrip.Y + targetGrip.H/2
	tester.Press(x, y)
	for step := 1; step <= 8; step++ {
		tester.Move(x, y+(target-y)*float32(step)/8)
		tester.Advance(16 * time.Millisecond)
		tester.Frame()
		if step == 5 {
			// The other rows have made room for the lifted one.
			tester.Advance(300 * time.Millisecond)
			tester.Frame()
			save("drag")
		}
	}
	tester.Release(x, target)
	settle("search services reordered by drag", func() bool {
		first, second, third := find("TinyFish"), find("Tavily"), find("Exa")
		handles := named(ui.RoleButton, "拖动排序")
		return first.Y < second.Y && second.Y < third.Y && len(handles) == len(services) && !handles[firstService].Disabled
	})
	rest()
	reordered := slices.Clone(services)
	reordered[firstService], reordered[firstService+1], reordered[firstService+2] = protocol.SearchDriverTinyfish, protocol.SearchDriverTavily, protocol.SearchDriverExa
	if saved := order(); !slices.Equal(saved, reordered) {
		t.Fatalf("search settings: the Node saved %v after dragging Exa below Tavily, want %v", saved, reordered)
	}
	save("reordered")
	// The keyboard moves it back: Space lifts the row of the focused
	// handle, Up moves it, Space drops it.
	lifted := named(ui.RoleButton, "拖动排序")[firstService+2].Bounds
	tester.ClickAt(lifted.X+lifted.W/2, lifted.Y+lifted.H/2)
	for _, key := range []ui.Key{ui.KeySpace, ui.KeyUp, ui.KeyUp, ui.KeySpace} {
		tester.Key(0, key)
		tester.Advance(16 * time.Millisecond)
		tester.Frame()
	}
	settle("search services reordered from the keyboard", func() bool {
		first, second, third := find("Exa"), find("TinyFish"), find("Tavily")
		handles := named(ui.RoleButton, "拖动排序")
		return first.Y < second.Y && second.Y < third.Y && len(handles) == len(services) && !handles[firstService].Disabled
	})
	rest()
	if saved := order(); !slices.Equal(saved, services) {
		t.Fatalf("search settings: the Node saved %v after the keyboard moved Exa back, want %v", saved, services)
	}
	save("restored")
	// A core page beside it, for the same rows.
	runtime.App.Settings.Open(settings.General)
	rest()
	save("core-general")
}

// auditContextRing shows a session's context meter, the core's, in
// Chinese: directly left of the model chip, with an accessible title and
// state, a summary on hover that can be pinned and expanded, and no meter on
// a draft. The local package contributes no extra context ring. It is
// a step of TestPluginUIIntegration; SAILRY_UI_SHOTS receives the light and
// the dark frame of the summary and of the details.
func auditContextRing(t *testing.T, runtime *Runtime, tester *ui.Tester, session string, wait func(string, func() bool)) {
	t.Helper()
	// metered says the selected conversation's composer is given the meter;
	// rings are the packages' indicators beside it.
	var metered bool
	var rings []composer.Indicator
	project := func() {
		runtime.enqueue(func(view *ui.Context) {
			if conversation := runtime.App.Data.Find(runtime.App.Sidebar.Selected); conversation != nil {
				comp := runtime.App.Composer(conversation.ID)
				projected, _ := runtime.conversation(view, conversation, comp)
				metered, rings = comp.Indicator != nil, projected.Indicators
			}
		})
	}
	tester.SetLocale("zh-CN")
	prefix := i18n.In("zh-CN", "meter_title") + " · "
	find := func() (ui.AccessInfo, bool) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && strings.HasPrefix(info.Name, prefix) {
				return info, true
			}
		}
		return ui.AccessInfo{}, false
	}
	var meter ui.AccessInfo
	wait("the session's context meter", func() bool {
		project()
		var shown bool
		meter, shown = find()
		return runtime.App.Sidebar.Selected == session && metered && shown
	})
	if len(rings) != 0 {
		t.Errorf("the composer has package rings beside the meter: %+v", rings)
	}
	chipPrefix := i18n.In("zh-CN", "composer_model_settings") + ": "
	var chip ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && strings.HasPrefix(info.Name, chipPrefix) {
			chip = info
		}
	}
	if chip.Name == "" {
		t.Fatalf("no model chip: %q", tester.Texts())
	}
	if meter.Bounds.H > 24 || meter.Bounds.W > kit.RingSize+8 {
		t.Errorf("the meter is %v, want the Kit ring within the toolbar's 24", meter.Bounds)
	}
	if gap := chip.Bounds.X - (meter.Bounds.X + meter.Bounds.W); gap < 0 || gap > 12 {
		t.Errorf("the meter ends %v before the model chip, want it directly left", gap)
	}
	if middle, row := meter.Bounds.Y+meter.Bounds.H/2, chip.Bounds.Y+chip.Bounds.H/2; middle < row-2 || middle > row+2 {
		t.Errorf("the meter's middle %v is off the model chip's %v", middle, row)
	}
	showDetails := i18n.In("zh-CN", "meter_show_details")
	hideDetails := i18n.In("zh-CN", "meter_hide_details")
	x, y := meter.Bounds.X+meter.Bounds.W/2, meter.Bounds.Y+meter.Bounds.H/2
	if _, open := tester.AccessByRole(ui.RoleButton, showDetails); open {
		t.Fatal("the meter summary was open before hovering")
	}
	tester.Move(x, y)
	wait("the meter summary opens on hover", func() bool {
		return tester.HasText(i18n.In("zh-CN", "meter_window_limit")) && tester.HasText(showDetails)
	})
	settle := func() {
		for range 4 {
			tester.Advance(80 * time.Millisecond)
			tester.Frame()
		}
	}
	tester.ClickAt(x, y)
	tester.Move(1, 1)
	settle()
	details, ok := tester.AccessByRole(ui.RoleButton, showDetails)
	if !ok {
		t.Fatalf("clicking the hovered meter did not pin its summary: %q", tester.Texts())
	}
	shoot := func(name string) {
		dir := os.Getenv("SAILRY_UI_SHOTS")
		if dir == "" {
			return
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, dark := range []bool{false, true} {
			tester.SetDark(dark)
			settle()
			file := name + "-zh-CN-light.png"
			if dark {
				file = name + "-zh-CN-dark.png"
			}
			f, err := os.Create(filepath.Join(dir, file))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(f, tester.Image())
			f.Close()
			if err != nil {
				t.Fatal(err)
			}
		}
		tester.SetDark(false)
	}
	shoot("session-meter-summary")
	tester.ClickAt(details.Bounds.X+details.Bounds.W/2, details.Bounds.Y+details.Bounds.H/2)
	settle()
	if _, expanded := tester.AccessByRole(ui.RoleButton, hideDetails); !expanded {
		t.Fatalf("Show details did not expand the meter: %q", tester.Texts())
	}
	shoot("session-meter-details")
	tester.Key(0, ui.KeyEscape)
	settle()
	_, summary := tester.AccessByRole(ui.RoleButton, showDetails)
	_, expanded := tester.AccessByRole(ui.RoleButton, hideDetails)
	if summary || expanded {
		t.Fatalf("Escape did not close the meter: %q", tester.Texts())
	}
	auditDraftRing(runtime, tester, session, wait, project, &metered, find)
}

// auditDraftRing checks that a draft, which has no session, is given no
// meter and shows none, and goes back to the session.
func auditDraftRing(runtime *Runtime, tester *ui.Tester, session string, wait func(string, func() bool), project func(), metered *bool, find func() (ui.AccessInfo, bool)) {
	tester.Move(1, 1)
	runtime.App.Sidebar.NewChat()
	wait("the draft without a meter", func() bool {
		project()
		_, shown := find()
		return strings.HasPrefix(runtime.App.Sidebar.Selected, "draft/") && !*metered && !shown
	})
	runtime.OpenSession(session)
	tester.SetLocale("en")
	wait("the session again", func() bool { return runtime.App.Sidebar.Selected == session })
}
