package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The rail's host menu opens the host's page: its monitor in the main
// column, the summary column closed until its toggle opens it; the update
// dialog plays its phases; ⌘W closes the page.
func TestHostPage(t *testing.T) {
	a, tt := newTester(t)
	a.openHost("remote", false)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, s := range []string{"workspace_host_status", "host_utilization", "host_storage", "workspace_running", "projects", "port_title"} {
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
	clickItem(t, lt, i18n.In("zh-CN", "hosts"))
	lt.Frame()
	if !lt.HasText("settings") || !lt.HasText(i18n.In("zh-CN", "local_host_name")) {
		t.Errorf("live host menu: %q", lt.Texts())
	}
	t.Run("rail menu", testHostMenu)
	t.Run("layout", testHostLayout)
	t.Run("demo", testHostDemo)
	t.Run("sample links", testSampleLinks)
}

// The host page is laid out as the reference's. The sessions' side column
// stays beside it, and the hosts are the rail picker's, which lists every
// one. The header shows once, then the monitor's cards, the running agents,
// the projects and the runtime, in that order. The summary column lists
// only the volumes that hold the user's files.
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
			en("workspace_running"), en("projects"),
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
		// The runtime section is gone (docs/removed-ui.md): neither its title
		// nor a label of its grid shows. The paired host's versions are not
		// known, so its line is the platform alone.
		for _, text := range []string{"Runtime", "Host type", "Worktrees", "System version", "Time zone", en("host_version"), en("host_protocol")} {
			if shown[text] != 0 {
				t.Errorf("the page shows %q of the removed runtime section: %q", text, tt.Texts())
			}
		}
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
	t.Run("hosts in the rail", func(t *testing.T) {
		// The reference lists hosts in the rail's picker, not in the side
		// column: the other host shows once it opens.
		other := en("local_host_name")
		if tt.HasText(other) {
			t.Fatalf("%q shows before the picker opens: %q", other, tt.Texts())
		}
		clickItem(t, tt, en("hosts"))
		tt.Frame()
		for _, x := range a.Data.Hosts {
			if !tt.HasText(en(x.Name)) {
				t.Errorf("the picker does not list %q: %q", en(x.Name), tt.Texts())
			}
		}
		if !tt.HasText(en("host_add")) {
			t.Errorf("the picker has no Add host: %q", tt.Texts())
		}
	})
}

// The host picker's right-click menu lists the chosen host's port forwards,
// offers the update of a host that has one and removes a paired host's
// pairing after its confirmation; a host without a pairing has no such item.
func testHostMenu(t *testing.T) {
	a, tt := newTester(t)
	menu := func() []string {
		t.Helper()
		info, ok := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "hosts"))
		if !ok {
			t.Fatalf("no host picker: %q", tt.Texts())
		}
		tt.RightClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		tt.Frame()
		return tt.Menu()
	}
	choose := func(path ...string) {
		t.Helper()
		menu()
		if err := tt.ChooseMenuItem(path...); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Frame()
	}
	revoke, update := i18n.In("en", "live_revoke"), i18n.In("en", "sidebar_host_update")+" "+i18n.In("en", "remote_host_name")
	if items := menu(); slices.Contains(items, revoke) || !slices.Contains(items, update) || !slices.Contains(items, i18n.In("en", "port_mappings")) {
		t.Fatalf("this computer's menu: %q", items)
	}
	if err := tt.ChooseMenuItem(update); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	tt.Frame()
	if a.Page != HostPage || a.Host != "remote" || !tt.HasText(i18n.In("en", "update_check")) {
		t.Fatalf("Update: page %d host %q %q", a.Page, a.Host, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "close"))
	tt.Frame()
	a.ClosePage()
	tt.Frame()
	choose(i18n.In("en", "port_mappings"), i18n.In("en", "port_title"))
	if a.Page != HostPage || !tt.HasText(i18n.In("en", "port_add")) {
		t.Fatalf("Port forwarding: page %d %q", a.Page, tt.Texts())
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	menu()
	if err := tt.ChooseMenuItem(i18n.In("en", "port_mappings"), "8080 → 127.0.0.1:18080"); err == nil {
		t.Error("a forward that is not listening could be opened")
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	a.ClosePage()

	revoked := 0
	a.Hooks.HostActions = func(id string) host.Actions {
		if id != "remote" {
			return host.Actions{}
		}
		return host.Actions{Revoke: func() { revoked++ }}
	}
	tt.Frame()
	choose(revoke)
	confirm := i18n.In("en", "live_revoke_confirm", "name", i18n.In("en", "remote_host_name"))
	if a.Page != HostPage || !tt.HasText(confirm) {
		t.Fatalf("Remove pairing: page %d %q", a.Page, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "settings_cancel"))
	tt.Frame()
	if revoked != 0 || tt.HasText(confirm) {
		t.Fatalf("cancel: %d revoked %q", revoked, tt.Texts())
	}
	// The page has no Remove pairing button (docs/removed-ui.md): the menu
	// is the one way to ask.
	if info, ok := tt.AccessByRole(ui.RoleButton, revoke); ok {
		t.Errorf("the host page has a Remove pairing button at %v", info.Bounds)
	}
	choose(revoke)
	if !tt.HasText(confirm) {
		t.Fatalf("Remove pairing again: %q", tt.Texts())
	}
	var last ui.Rect
	for _, info := range tt.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == revoke {
			last = info.Bounds
		}
	}
	tt.ClickAt(last.X+last.W/2, last.Y+last.H/2)
	tt.Frame()
	tt.Frame()
	if revoked != 1 || tt.HasText(confirm) {
		t.Fatalf("confirm: %d revoked %q", revoked, tt.Texts())
	}
	// A removal that runs disables the item.
	h, _ := a.hostPage()
	h.Revoking = true
	tt.Frame()
	menu()
	if err := tt.ChooseMenuItem(revoke); err == nil {
		t.Error("Remove pairing could be chosen while it runs")
	}
}

// Demo mode says nowhere that its data is a sample: over the preview's
// hosts, neither the host page, its summary column, its update dialog nor
// the rail's host picker shows a preview's note, and the page's actions
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
	// The rail's picker names the hosts and nothing else.
	picker, ok := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "hosts"))
	if !ok {
		t.Fatalf("no host picker: %q", tt.Texts())
	}
	tt.ClickAt(picker.Bounds.X+picker.Bounds.W/2, picker.Bounds.Y+picker.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "host_add")) {
		t.Fatalf("the host picker did not open: %q", tt.Texts())
	}
	marked("the host picker")
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
	for _, text := range []string{"Runtime", "Host type", "Worktrees", "System version", "Time zone", en("host_version"), en("host_protocol")} {
		if tt.HasText(text) {
			t.Errorf("%s shows %q of the removed runtime section in Demo mode", id, text)
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
