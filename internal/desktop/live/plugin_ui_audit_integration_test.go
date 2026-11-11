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
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
	// The Search settings page is exercised last, in Chinese and dark: its
	// drags and saves come after every page was drawn at rest.
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	runtime.App.Mode = theme.Dark
	settle("package locale zh-CN", func() bool { return runtime.packageLocale() == "zh-CN" })
	for section, entry := range runtime.pluginViews.settings {
		if entry.info.Summary.Name == "web-search" {
			runtime.App.Settings.Open(section)
			runtime.App.Navigate(shell.SettingsPage)
			rest()
			auditSearchSettings(t, ctx, runtime, tester, do, settle, rest, func(step string) { save("web-search-" + step + "-zh-CN-dark") })
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

// auditSearchSettings asserts the web-search package's Settings page in
// Chinese: its rows keep the application's row metrics, the overflow menu is
// an icon button, the services reorder by dragging a handle and from the
// keyboard with the order saved on the Node, no arrow buttons remain and the
// brand icons no icon set knows yet leave the rows without one. It is a step
// of TestPluginUIIntegration.
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
	named := func(name string) (found []ui.AccessInfo) {
		for _, info := range tester.AccessAll() {
			if info.Name == name {
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
	if len(services) < 3 || services[0] != "exa" || services[1] != "tinyfish" || services[2] != "tavily" {
		t.Fatalf("search settings: unexpected default order %v", services)
	}
	// The application's rows: a title alone is the minimum row height, and
	// a described row is its padding, title, gap and description. The
	// core row over the page (enable the package) starts at the same edge.
	enable, service, fallback := find("启用插件"), find("默认服务"), find("使用备用服务")
	// The Kit select of the first row is a fraction over its 32 control.
	if pitch := fallback.Y - service.Y; pitch < kit.RowMinHeight || pitch >= kit.RowMinHeight+1 || enable.X != service.X || enable.H != service.H {
		t.Errorf("search settings: rows %v and %v are not %v apart, or off the core row %v", service, fallback, float32(kit.RowMinHeight), enable)
	}
	// Each described row is as high as its own two lines: a detail mixing
	// scripts is a fraction higher than one in a single script.
	exa, quota, tinyfish, key, tavily := find("Exa"), find("匿名额度"), find("TinyFish"), find("需要 API 密钥 · 免费额度"), find("Tavily")
	near := func(got, want float32) bool { return got > want-0.01 && got < want+0.01 }
	if !near(quota.Y-exa.Y-exa.H, kit.TitleGap) || !near(tinyfish.Y-exa.Y, 2*kit.RowPad+exa.H+kit.TitleGap+quota.H) ||
		!near(key.Y-tinyfish.Y-tinyfish.H, kit.TitleGap) || !near(tavily.Y-tinyfish.Y, 2*kit.RowPad+tinyfish.H+kit.TitleGap+key.H) || exa.H != service.H {
		t.Errorf("search settings: service rows Exa %v, its detail %v, TinyFish %v, its detail %v, Tavily %v are off the row metrics", exa, quota, tinyfish, key, tavily)
	}
	// Until the brand icon set is merged every "ailogo:" name is unknown:
	// the page draws without an icon and the texts stay aligned.
	if exa.X != tinyfish.X || exa.X != tavily.X {
		t.Errorf("search settings: service titles are not aligned: %v %v %v", exa, tinyfish, tavily)
	}
	// One handle and one overflow icon button a service; no arrows, and
	// the word of the menu only as its name.
	handles, more := named("拖动排序"), named("更多")
	shown := 0
	for _, text := range tester.Texts() {
		if text == "更多" {
			shown++
		}
	}
	if len(handles) != len(services) || len(more) != len(services) || shown != len(services) || len(named("上移"))+len(named("下移")) != 0 {
		t.Fatalf("search settings: %d handles, %d overflow menus, %d overflow texts, arrows %d for %d services",
			len(handles), len(more), shown, len(named("上移"))+len(named("下移")), len(services))
	}
	if more[0].Role != ui.RoleMenuButton || more[0].Bounds.W != 28 || more[0].Bounds.H != 28 {
		t.Errorf("search settings: the overflow menu is %v with role %v, want a 28 icon button", more[0].Bounds, more[0].Role)
	}
	// Drag Exa by its handle below Tavily.
	grip := handles[0].Bounds
	x, y := grip.X+grip.W/2, grip.Y+grip.H/2
	target := y + 2*(tavily.Y-tinyfish.Y)
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
		return first.Y < second.Y && second.Y < third.Y && len(named("拖动排序")) == len(services) && !named("拖动排序")[0].Disabled
	})
	rest()
	if saved := order(); saved[0] != "tinyfish" || saved[1] != "tavily" || saved[2] != "exa" || !slices.Equal(saved[3:], services[3:]) {
		t.Fatalf("search settings: the Node saved %v after dragging Exa below Tavily from %v", saved, services)
	}
	save("reordered")
	// The keyboard moves it back: Space lifts the row of the focused
	// handle, Up moves it, Space drops it.
	lifted := named("拖动排序")[2].Bounds
	tester.ClickAt(lifted.X+lifted.W/2, lifted.Y+lifted.H/2)
	for _, key := range []ui.Key{ui.KeySpace, ui.KeyUp, ui.KeyUp, ui.KeySpace} {
		tester.Key(0, key)
		tester.Advance(16 * time.Millisecond)
		tester.Frame()
	}
	settle("search services reordered from the keyboard", func() bool {
		first, second, third := find("Exa"), find("TinyFish"), find("Tavily")
		return first.Y < second.Y && second.Y < third.Y && !named("拖动排序")[0].Disabled
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
	for _, mounted := range runtime.pluginViews.mounts {
		if mounted.scope.Package.Name == "web-search" && mounted.failure != nil {
			t.Fatalf("search settings: %v", mounted.failure)
		}
	}
}

// kitPageColumn is the content width of kit.Page: its 800 column less its
// padding of 24 a side.
const kitPageColumn = 752

// auditContextRing shows a session's context ring, the statistics package's
// composer indicator, in Chinese: directly left of the model chip, its hint as
// the tooltip, no tooltip while its usage is unknown (auditUnknownRing), and
// gone on a draft. It is a step of TestPluginUIIntegration;
// SAILRY_UI_SHOTS receives the light and the dark frame.
func auditContextRing(t *testing.T, runtime *Runtime, tester *ui.Tester, session string, wait func(string, func() bool)) {
	t.Helper()
	// rings are the indicators the selected conversation's composer is given.
	var rings []composer.Indicator
	project := func() {
		runtime.enqueue(func(view *ui.Context) {
			if conversation := runtime.App.Data.Find(runtime.App.Sidebar.Selected); conversation != nil {
				projected, _ := runtime.conversation(view, conversation, runtime.App.Composer(conversation.ID))
				rings = projected.Indicators
			}
		})
	}
	tester.SetLocale("zh-CN")
	var ring ui.AccessInfo
	wait("the session's context ring", func() bool {
		project()
		if runtime.App.Sidebar.Selected != session || len(rings) != 1 {
			return false
		}
		// A ring whose usage is unknown has no hint, so no name to find.
		if rings[0].Hint == "" {
			return true
		}
		var shown bool
		ring, shown = tester.Access(rings[0].Hint)
		return shown
	})
	hint := rings[0].Hint
	if hint == "" {
		auditUnknownRing(t, tester)
		auditDraftRing(runtime, tester, session, wait, project, &rings, hint)
		return
	}
	if strings.ContainsFunc(hint, unicode.IsLetter) && !strings.ContainsFunc(hint, func(r rune) bool { return unicode.Is(unicode.Han, r) }) {
		t.Errorf("the ring's hint is not Chinese: %q", hint)
	}
	prefix := i18n.In("zh-CN", "composer_model_settings") + ": "
	var chip ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && strings.HasPrefix(info.Name, prefix) {
			chip = info
		}
	}
	if chip.Name == "" {
		t.Fatalf("no model chip: %q", tester.Texts())
	}
	if ring.Bounds.H > 24 || ring.Bounds.W > kit.RingSize+8 {
		t.Errorf("the ring is %v, want the Kit ring within the toolbar's 24", ring.Bounds)
	}
	if gap := chip.Bounds.X - (ring.Bounds.X + ring.Bounds.W); gap < 0 || gap > 12 {
		t.Errorf("the ring ends %v before the model chip, want it directly left", gap)
	}
	if middle, row := ring.Bounds.Y+ring.Bounds.H/2, chip.Bounds.Y+chip.Bounds.H/2; middle < row-2 || middle > row+2 {
		t.Errorf("the ring's middle %v is off the model chip's %v", middle, row)
	}
	// The ring's own name counts as a text: the tooltip is one more.
	shows := func() (count int) {
		for _, text := range tester.Texts() {
			if text == hint {
				count++
			}
		}
		return
	}
	before := shows()
	tester.Move(ring.Bounds.X+ring.Bounds.W/2, ring.Bounds.Y+ring.Bounds.H/2)
	tester.Frame()
	tester.Advance(2 * time.Second)
	tester.Frame()
	tester.Frame()
	if shows() != before+1 {
		t.Errorf("hovering the ring shows %q %d times, was %d: no tooltip", hint, shows(), before)
	}
	if dir := os.Getenv("SAILRY_UI_SHOTS"); dir != "" {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, dark := range []bool{false, true} {
			tester.SetDark(dark)
			for range 4 {
				tester.Advance(16 * time.Millisecond)
				tester.Frame()
			}
			name := "session-ring-header-zh-CN-light.png"
			if dark {
				name = "session-ring-header-zh-CN-dark.png"
			}
			file, err := os.Create(filepath.Join(dir, name))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(file, tester.Image())
			file.Close()
			if err != nil {
				t.Fatal(err)
			}
		}
		tester.SetDark(false)
	}
	auditDraftRing(runtime, tester, session, wait, project, &rings, hint)
}

// auditUnknownRing hovers the place of a ring whose usage is unknown,
// directly left of the model chip: the ring stays and no tooltip shows, the
// statistics package's placeholder "0" least of all.
func auditUnknownRing(t *testing.T, tester *ui.Tester) {
	t.Helper()
	prefix := i18n.In("zh-CN", "composer_model_settings") + ": "
	var chip ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && strings.HasPrefix(info.Name, prefix) {
			chip = info
		}
	}
	if chip.Name == "" {
		t.Fatalf("no model chip: %q", tester.Texts())
	}
	rest := slices.Clone(tester.Texts())
	// The ring ends the toolbar's gap of 8 before the chip, 4 at each side.
	tester.Move(chip.Bounds.X-8-4-kit.RingSize/2, chip.Bounds.Y+chip.Bounds.H/2)
	tester.Frame()
	tester.Advance(2 * time.Second)
	tester.Frame()
	tester.Frame()
	if under := tester.Texts(); !slices.Equal(under, rest) {
		t.Errorf("the unknown ring shows a tooltip: %q at rest, %q under the pointer", rest, under)
	}
}

// auditDraftRing checks that a draft, which has no session, is given no ring
// and shows none, and goes back to the session.
func auditDraftRing(runtime *Runtime, tester *ui.Tester, session string, wait func(string, func() bool), project func(), rings *[]composer.Indicator, hint string) {
	tester.Move(1, 1)
	runtime.App.Sidebar.NewChat()
	wait("the draft without a ring", func() bool {
		project()
		shown := false
		if hint != "" {
			_, shown = tester.Access(hint)
		}
		return strings.HasPrefix(runtime.App.Sidebar.Selected, "draft/") && len(*rings) == 0 && !shown
	})
	runtime.OpenSession(session)
	tester.SetLocale("en")
	wait("the session again", func() bool { return runtime.App.Sidebar.Selected == session })
}
