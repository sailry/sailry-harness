package shell

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Connections opens the host's page: its monitor in the main
// column, the summary column closed until its toggle opens it; the update
// dialog plays its phases; ⌘W closes the page.
func TestHostPage(t *testing.T) {
	a, tt := newTester(t)
	a.openHost("remote", false)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, s := range []string{"workspace_host_status", "host_utilization", "host_storage", "workspace_running", "port_title"} {
		if !tt.HasText(i18n.In("en", s)) {
			t.Errorf("no %s: %q", s, tt.Texts())
		}
	}
	snapshot(t, tt, "host-remote")
	clickButton(t, tt, i18n.In("en", "host_update"))
	tt.Frame()
	clickButton(t, tt, i18n.In("en", "update_check"))
	tt.Frame()
	if h, _ := a.hostPage(); h.Update.Phase != "update_available" || !tt.HasText(i18n.In("en", "update_download")) {
		t.Fatalf("check: %+v %q", h.Update, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "close"))
	tt.Frame()
	// The page is the monitor, so the summary column starts closed, as the
	// reference's (panel_open of Page::Host); the toggle opens and closes it.
	if a.Metrics || tt.HasText(i18n.In("en", "metrics_title")) {
		t.Error("the page opened with its summary column")
	}
	a.togglePanel()
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !a.Metrics || !tt.HasText(i18n.In("en", "metrics_title")) {
		t.Errorf("the toggle did not open the summary: %q", tt.Texts())
	}
	a.togglePanel()
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if a.Metrics || tt.HasText(i18n.In("en", "metrics_title")) {
		t.Error("the metrics stayed")
	}
	a.ClosePage()
	tt.Frame()
	if a.Page != ConversationPage {
		t.Errorf("page %d", a.Page)
	}
	// A device's own name is not a key of the catalog: a live host named
	// like one shows its name; the preview's hosts and the local one are
	// keys.
	live := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{
		{ID: "local", Name: "local_host_name", Local: true}, {ID: "peer", Name: "settings"},
	}}})
	lt := ui.NewTesterWith(live.View, 1280, 820, opaque)
	lt.SetLocale("zh-CN")
	lt.Frame()
	live.SetHosts(map[string]*host.Host{
		"peer":  {ID: "peer", Name: "settings", Live: true, Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}},
		"local": {ID: "local", Name: "local_host_name", Local: true, Live: true, Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}},
	})
	live.OpenHost("peer")
	lt.Frame()
	if !lt.HasText("settings") {
		t.Errorf("live host name: %q", lt.Texts())
	}
	live.OpenHost("local")
	lt.Frame()
	if !lt.HasText(i18n.In("zh-CN", "local_host_name")) {
		t.Errorf("local host name: %q", lt.Texts())
	}

	t.Run("device menu", testHostMenu)
	t.Run("layout", testHostLayout)
	t.Run("demo", testHostDemo)
	t.Run("sample links", testSampleLinks)
}

func TestSidebarHostListOpensMonitor(t *testing.T) {
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{
		{ID: "local", Name: "local_host_name", Local: true},
		{ID: "remote", Name: "Build node"},
	}}}
	a := NewLive(data)
	a.Sidebar.Actions = &sidebar.Actions{OpenHost: a.OpenHost}
	a.SetHosts(map[string]*host.Host{
		"local":  {ID: "local", Name: "local_host_name", Local: true, Live: true, Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}},
		"remote": {ID: "remote", Name: "Build node", Live: true, Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}},
	})
	tester := ui.NewTesterWith(a.View, 900, 700, opaque)
	tester.SetLocale("en")
	tester.Frame()
	if !tester.HasText(i18n.In("en", "hosts")) || !tester.HasText("Build node") {
		t.Fatalf("host list is missing: %q", tester.Texts())
	}
	selected := a.Sidebar.Selected
	row, ok := tester.AccessByRole(ui.RoleButton, "Build node")
	if !ok {
		t.Fatal("remote host row is missing")
	}
	tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	tester.Frame()
	if a.Page != HostPage || a.Host != "remote" {
		t.Fatalf("host click opened page %d for %q", a.Page, a.Host)
	}
	if a.Sidebar.Selected != selected {
		t.Fatal("opening a host changed the selected conversation")
	}
	if row, ok := tester.AccessByRole(ui.RoleButton, "Build node"); !ok || !row.Checked {
		t.Fatalf("host row is not selected: %+v", row)
	}
}

// The host page is laid out as the reference's. The sessions' side column
// stays beside it, and Connections lists the devices. The header shows once, then the monitor's cards and the running
// agents, in that order; the page has neither a projects nor a runtime
// section. The summary column lists only the volumes that hold the user's
// files.
func testHostLayout(t *testing.T) {
	a, tt := newTester(t)
	en := func(key string) string { return i18n.In("en", key) }
	count := func() map[string]int {
		out := map[string]int{}
		for _, text := range tt.Texts() {
			out[text]++
		}
		return out
	}
	// Tall enough for the whole page.
	tt.SetSize(1280, 2200)
	a.openHost("remote", false)
	tt.Frame()
	settle(tt)
	h, _ := a.hostPage()
	if h.Metrics == nil {
		t.Fatalf("no sample: %q", tt.Texts())
	}

	t.Run("side column", func(t *testing.T) {
		if !a.HasNavigation() || !a.NavShown() || !sessionsShown(tt) {
			t.Fatalf("the host page has no side column: navigation %v, shown %v, sessions %v", a.HasNavigation(), a.NavShown(), sessionsShown(tt))
		}
		// The column is past the rail and before the page.
		name, ok := tt.Find(en("workspace_host_status"))
		if !ok || name.X < RailWidth+NavWidth {
			t.Errorf("the page starts at %v, want past the %v side column", name, RailWidth+NavWidth)
		}
	})
	t.Run("sections in order", func(t *testing.T) {
		order := []string{
			en("workspace_host_status"), en("remote_host_detail"), en("host_uptime"), en("host_node_uptime"),
			en("metrics_cpu"), en("metrics_memory"), en("host_utilization"), en("metrics_network"), en("metrics_disks"),
			en("host_storage"), en("host_process_search"), en("host_pid"),
			en("workspace_running"),
		}
		texts, at := tt.Texts(), 0
		for _, want := range order {
			i := slices.Index(texts[at:], want)
			if i < 0 {
				t.Fatalf("no %q in its place: %q", want, texts)
			}
			at += i + 1
		}
	})
	t.Run("header once", func(t *testing.T) {
		shown := count()
		for _, key := range []string{"workspace_host_status", "remote_host_detail", "host_uptime", "host_node_uptime"} {
			if shown[en(key)] != 1 {
				t.Errorf("%s shows %d times: %q", key, shown[en(key)], tt.Texts())
			}
		}
		// The runtime section is gone (AGENTS.md, Removed UI): neither its title
		// nor a label of its grid shows. The paired host's versions are not
		// known, so its line is the platform alone.
		for _, text := range []string{"Runtime", "Host type", "Worktrees", "System version", "Time zone", en("host_version"), en("host_protocol")} {
			if shown[text] != 0 {
				t.Errorf("the page shows %q of the removed runtime section: %q", text, tt.Texts())
			}
		}
	})
	t.Run("no projects section", func(t *testing.T) {
		hostNoProjects(t, tt, "the paired host")
	})
	t.Run("summary volumes", func(t *testing.T) {
		// The Node lists only real volumes, so the page and its summary
		// show every one of a sample.
		real := h.Metrics.Disks[0].Mount
		disks := h.Metrics.Disks
		h.Metrics.Disks = append(disks[:len(disks):len(disks)],
			host.Disk{Mount: "/Volumes/Backup", FileSystem: "apfs", Total: 1 << 40, Available: 1 << 39})
		tt.Frame()
		before := count()
		if before[real] == 0 || before["/Volumes/Backup"] == 0 {
			t.Fatalf("the page's volumes: %q", tt.Texts())
		}
		a.togglePanel()
		tt.Frame()
		settle(tt)
		after := count()
		if !a.Metrics || after[en("metrics_title")] == 0 {
			t.Fatalf("the summary did not open: %q", tt.Texts())
		}
		if after[real] <= before[real] || after["/Volumes/Backup"] <= before["/Volumes/Backup"] {
			t.Errorf("the summary's volumes: %q %d then %d, /Volumes/Backup %d then %d", real, before[real], after[real], before["/Volumes/Backup"], after["/Volumes/Backup"])
		}
		a.togglePanel()
		tt.Frame()
		settle(tt)
		h.Metrics.Disks = disks
	})
	t.Run("device access stays in Connections", func(t *testing.T) {
		if _, ok := tt.AccessByRole(ui.RoleMenuButton, en("hosts")); ok {
			t.Fatal("global host picker is still present")
		}
		a.Navigate(SettingsPage)
		a.Settings.Open(settings.Connections)
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleButton, en("settings_add")); !ok {
			t.Fatal("Connections lost Add host")
		}
		selected := a.Sidebar.Selected
		a.Settings.ConnectionActions.Inspect(viewmodel.Device{ID: "remote", Execution: true})
		tt.Frame()
		if a.Page != HostPage || a.Host != "remote" {
			t.Fatalf("device monitor target: page %d host %q", a.Page, a.Host)
		}
		if a.Sidebar.Selected != selected {
			t.Fatal("opening a monitor changed the selected project session")
		}
		a.OpenHost("unknown")
		if a.Host != "remote" {
			t.Fatal("unknown device replaced the monitor target")
		}

	})
}

// The monitored device's More menu keeps ports, updates, editing and revocation.
func testHostMenu(t *testing.T) {
	a, tt := newTester(t)
	en := func(key string) string { return i18n.In("en", key) }
	a.OpenHost("local")
	tt.Frame()
	open := func() { t.Helper(); openHostMore(t, tt) }
	open()
	if _, ok := tt.AccessByRole(ui.RoleMenuItem, en("live_revoke")); ok {
		t.Fatal("local monitor offers revocation")
	}
	if _, ok := tt.AccessByRole(ui.RoleMenuItem, en("host_update")); ok {
		t.Fatal("local monitor offers remote update")
	}
	clickItem(t, tt, en("port_mappings"))
	settle(tt)
	// A closed forward still lists, so it can be edited or deleted.
	if info, ok := tt.AccessByRole(ui.RoleMenuItem, "8080 → 127.0.0.1:18080"); !ok || info.Disabled {
		t.Fatal("the closed port forward is not listed")
	}
	clickItem(t, tt, en("port_add"))
	settle(tt)
	if !tt.HasText(en("port_remote")) {
		t.Fatal("New forward did not open its dialog")
	}
	tt.Key(0, ui.KeyEscape)
	settle(tt)
	a.OpenHost("remote")
	settle(tt)
	open()
	clickItem(t, tt, en("host_update"))
	settle(tt)
	if !tt.HasText(en("update_check")) {
		t.Fatalf("update dialog absent: %q", tt.Texts())
	}
	clickButton(t, tt, en("close"))
	settle(tt)
	edited := ""
	a.Hooks.EditHost = func(id string) { edited = id }
	open()
	clickItem(t, tt, en("connection_action_edit"))
	settle(tt)
	if edited != "remote" {
		t.Fatalf("edit targeted %q", edited)
	}
	revoked := 0
	a.Hooks.HostActions = func(id string) host.Actions {
		if id != "remote" {
			return host.Actions{}
		}
		return host.Actions{Revoke: func() { revoked++ }}
	}
	tt.Frame()
	chooseRevoke := func() { t.Helper(); open(); clickItem(t, tt, en("live_revoke")); settle(tt) }
	chooseRevoke()
	confirm := i18n.In("en", "live_revoke_confirm", "name", en("remote_host_name"))
	if !tt.HasText(confirm) {
		t.Fatalf("revocation confirmation absent: %q", tt.Texts())
	}
	clickButton(t, tt, en("settings_cancel"))
	settle(tt)
	if revoked != 0 || tt.HasText(confirm) {
		t.Fatal("cancel revoked the device or retained confirmation")
	}
	chooseRevoke()
	var last ui.Rect
	for _, info := range tt.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == en("live_revoke") {
			last = info.Bounds
		}
	}
	if last.W == 0 {
		t.Fatal("no revocation confirmation action")
	}
	tt.ClickAt(last.X+last.W/2, last.Y+last.H/2)
	settle(tt)
	if revoked != 1 || tt.HasText(confirm) {
		t.Fatalf("confirmed revocation: %d, %q", revoked, tt.Texts())
	}
	h, _ := a.hostPage()
	h.Revoking = true
	tt.Frame()
	open()
	if info, ok := tt.AccessByRole(ui.RoleMenuItem, en("live_revoke")); !ok || !info.Disabled {
		t.Fatal("revocation can be repeated while pending")
	}
}

func openHostMore(t *testing.T, tt *ui.Tester) {
	t.Helper()
	for _, info := range tt.AccessAll() {
		if info.Role == ui.RoleMenuButton && info.Name == i18n.In("en", "more") && info.Bounds.Y+info.Bounds.H <= HeaderHeight && info.Bounds.X > RailWidth {
			tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			settle(tt)
			return
		}
	}
	t.Fatalf("host header has no More menu: %q", tt.Texts())
}

// Demo mode says nowhere that its data is a sample: over the preview's
// hosts, neither the host page, its summary column, its update dialog nor
// the device menus show a preview's note, and the page's actions
// raise no preview notice.
func testHostDemo(t *testing.T) {
	a := New(preview.Load())
	a.Settings.Model.DemoMode = true
	tt := ui.NewTesterWith(a.View, 1440, 1960, opaque)
	tt.SetLocale("en")
	tt.Frame()
	// The preview's notes and every text of the page that names itself a
	// preview's. The preview's own example values (its version, platform
	// line and mount) are data, and stay.
	notes := map[string]string{}
	for _, key := range []string{
		"preview_notice", "workspace_host_status", "host_runtime_loading",
		"host_activity_hint", "metrics_preview", "metrics_worker", "metrics_indexer",
		"metrics_loading", "update_preview", "update_idle", "update_checking", "update_current", "update_available",
		"update_downloading", "update_verifying", "update_staged", "update_applying", "update_restart", "update_failed",
		"update_loading", "update_check", "update_download", "update_apply", "update_retry", "update_unavailable",
	} {
		notes[i18n.In("en", key)] = key
	}
	marked := func(where string) {
		t.Helper()
		for _, text := range tt.Texts() {
			if key := notes[text]; key != "" {
				t.Errorf("%s shows the preview's %s in Demo mode: %q", where, key, text)
			}
		}
		for _, info := range tt.AccessAll() {
			if key := notes[info.Name]; key != "" {
				t.Errorf("%s names a control by the preview's %s in Demo mode", where, key)
			}
		}
	}
	for _, id := range []string{"local", "remote"} {
		a.openHost(id, false)
		tt.Frame()
		// Before the first sample, and with it.
		marked(id + " while loading")
		tt.Advance(2 * time.Second)
		tt.Frame()
		tt.Frame()
		if !a.Demo() || !tt.HasText(i18n.In("en", "host_utilization")) {
			t.Fatalf("%s: the demo's host page: %q", id, tt.Texts())
		}
		marked(id)
		hostDemoHeader(t, tt, id)
		hostNoProjects(t, tt, id+" in Demo mode")
		hostDemoTip(t, tt, id)
		a.togglePanel()
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		if !tt.HasText(i18n.In("en", "metrics_title")) {
			t.Fatalf("%s: no summary: %q", id, tt.Texts())
		}
		marked(id + "'s summary")
		a.togglePanel()
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
	}
	// The remote host's update dialog, through its phases.
	clickButton(t, tt, i18n.In("en", "host_update"))
	tt.Frame()
	marked("the update dialog")
	clickButton(t, tt, i18n.In("en", "live_update_check"))
	tt.Frame()
	if h, _ := a.hostPage(); h.Update.Phase != "update_available" {
		t.Fatalf("the demo's check: %+v", h.Update)
	}
	marked("the checked update")
	clickButton(t, tt, i18n.In("en", "close"))
	tt.Frame()
	openHostMore(t, tt)
	marked("the device menu")
	if _, ok := tt.AccessByRole(ui.RoleMenuItem, i18n.In("en", "project_add")); !ok {
		t.Fatal("device menu lost Add project")
	}
	if _, ok := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "hosts")); ok {
		t.Fatal("demo retains a global host picker")
	}

}

// hostDemoHeader checks the demo's header line and the absence of the
// runtime section: this computer's line ends with its two versions after
// their labels, the paired host's, whose versions are unknown, is its
// platform alone, and neither page has the section's title or labels.
func hostDemoHeader(t *testing.T, tt *ui.Tester, id string) {
	t.Helper()
	en := func(key string) string { return i18n.In("en", key) }
	want := en("remote_host_detail")
	if id == "local" {
		want = en("local_host_detail") + " · " + en("host_version") + " 0.1.0-preview · " + en("host_protocol") + " v1"
	}
	if !tt.HasText(want) {
		t.Errorf("%s: no header line %q: %q", id, want, tt.Texts())
	}
	// A text of its own: the header's line holds the versions' labels.
	for _, text := range []string{"Runtime", "Host type", "Worktrees", "System version", "Time zone", en("host_version"), en("host_protocol")} {
		if slices.Contains(tt.Texts(), text) {
			t.Errorf("%s shows %q of the removed runtime section in Demo mode", id, text)
		}
	}
}

// hostNoProjects checks the absence of the host page's projects section
// (AGENTS.md, Removed UI): the sidebar beside the page names its own Projects
// group once and holds the one Add project button, and nothing after the
// page's header repeats either; the section's empty state, whose copy is
// out of the catalog, shows nowhere.
func hostNoProjects(t *testing.T, tt *ui.Tester, where string) {
	t.Helper()
	en := func(key string) string { return i18n.In("en", key) }
	titles := 0
	for _, text := range tt.Texts() {
		if text == en("projects") {
			titles++
		}
		if strings.Contains(text, "No projects on this host") {
			t.Errorf("%s shows the removed projects section's empty state %q", where, text)
		}
	}
	if titles > 1 {
		t.Errorf("%s names Projects %d times, want the sidebar's group alone: %q", where, titles, tt.Texts())
	}
	// The page is right of the side column: no Add project is in it.
	edge, ok := tt.Find(en("host_uptime"))
	if !ok {
		t.Fatalf("%s: no host page: %q", where, tt.Texts())
	}
	for _, info := range tt.AccessAll() {
		if info.Name == en("project_add") && info.Bounds.X >= RailWidth+NavWidth {
			t.Errorf("%s has an Add project control in the page at %v (the page's header is at %v)", where, info.Bounds, edge)
		}
	}
}

// hostDemoTip moves the pointer over the demo's utilization chart, at its
// last sample: the tip names the sample's time, which the axis under it
// shows once, and goes when the pointer leaves.
func hostDemoTip(t *testing.T, tt *ui.Tester, id string) {
	t.Helper()
	h := preview.HostPages(preview.Load())[id]
	clock := host.Clock(h.Metrics.At)
	times := func() int {
		n := 0
		for _, text := range tt.Texts() {
			if text == clock {
				n++
			}
		}
		return n
	}
	tick, ok := tt.Find(clock)
	if !ok || times() != 1 {
		t.Fatalf("%s: the utilization's last time shows %d times: %q", id, times(), tt.Texts())
	}
	tt.Move(tick.X+tick.W/2-4, tick.Y-60)
	tt.Frame()
	tt.Frame()
	if times() != 2 {
		t.Errorf("%s: no tip over the utilization: %q", id, tt.Texts())
	}
	tt.Move(4, 4)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if times() != 1 {
		t.Errorf("%s: the tip stays after the pointer left: %q", id, tt.Texts())
	}
}
