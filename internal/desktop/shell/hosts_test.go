package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The rail's host menu opens the host's page with its metrics in the right
// column; the update dialog plays its phases; ⌘W closes the page.
func TestHostPage(t *testing.T) {
	a, tt := newTester(t)
	a.openHost("remote", false)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, s := range []string{"workspace_host_status", "workspace_runtime", "workspace_running", "metrics_title", "port_title"} {
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
