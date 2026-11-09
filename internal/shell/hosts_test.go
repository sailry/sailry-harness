package shell

import (
	"testing"
	"time"

	"github.com/sailry/sailry-go/internal/i18n"
)

// The rail's host menu opens the host's page with its metrics in the right
// column; the update dialog plays its phases; ⌘W closes the page.
func TestHostPage(t *testing.T) {
	a, tt := newTester(t)
	a.openHost("remote", false)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, s := range []string{"workspace_host_status", "workspace_runtime", "workspace_running", "tunnels_title", "metrics_title", "port_title"} {
		if !tt.HasText(i18n.In("en", s)) {
			t.Errorf("no %s: %q", s, tt.Texts())
		}
	}
	if !tt.HasText(i18n.In("en", "tunnel_readonly")) {
		t.Error("the paired host's policy is editable")
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
	a.openHost("local", false)
	tt.SetSize(1280, 1400)
	tt.Frame()
	click(t, tt, i18n.In("en", "tunnel_edit"))
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "tunnel_policy_hint")) {
		t.Fatalf("no editor: %q", tt.Texts())
	}
}
