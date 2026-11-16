package settings

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestConfigurationFollowDialogShowsStatusAndRecovery(t *testing.T) {
	view := &ConfigurationFollowEditor{
		Device:  viewmodel.Device{Name: "Execution host"},
		Source:  "Controller",
		Targets: []ConfigurationTargetStatus{{ID: "target", Name: "Another host", Failure: "Host is offline"}},
		Issues:  []ConfigurationFollowIssue{{Category: "providers", ID: "account", State: "authorization", Message: "Sign in on this host", Authorize: true}},
	}
	m := &Model{FollowEditor: view}
	var retries, authorizations int
	m.FollowActions = &ConfigurationFollowActions{
		Refresh: func(editor *ConfigurationFollowEditor) {
			if editor != view {
				t.Fatal("retry selected another host")
			}
			retries++
		},
		Authorize: func(editor *ConfigurationFollowEditor, issue ConfigurationFollowIssue) {
			if editor != view || issue.ID != "account" {
				t.Fatal("authorization selected another host or account")
			}
			authorizations++
		},
	}
	tester := ui.NewTesterWith(func(c *ui.Context) { configurationFollowDialog(c, m) }, 800, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Advance(time.Second)
	tester.Frame()
	for _, text := range []string{"Controller", "Execution host", "Host is offline", "Sign in on this host"} {
		if !tester.HasText(text) {
			t.Fatalf("missing sync status %q: %q", text, tester.Texts())
		}
	}
	for _, item := range tester.AccessAll() {
		switch item.Role {
		case ui.RoleSwitch, ui.RoleComboBox, ui.RolePopUpButton, ui.RoleCheckBox:
			t.Fatalf("automatic configuration exposes a policy control: %+v", item)
		}
	}
	click := func(key string) {
		t.Helper()
		button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", key))
		if !ok || button.Disabled {
			t.Fatalf("missing recovery action %s: %q", key, tester.Texts())
		}
		tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		tester.Frame()
	}
	click("configuration_follow_authorize")
	click("settings_retry")
	if retries != 1 || authorizations != 1 {
		t.Fatalf("recovery actions were not preserved: retries=%d, authorizations=%d", retries, authorizations)
	}
	view.Pending = true
	tester.Frame()
	for _, key := range []string{"configuration_follow_authorize", "settings_retry", "close"} {
		button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", key))
		if !ok || !button.Disabled {
			t.Fatalf("pending operation left %s available", key)
		}
	}
	view.Pending, view.ReadFailed = false, true
	tester.Frame()
	if !tester.HasText(i18n.In("en", "configuration_follow_unavailable")) || tester.HasText(i18n.In("en", "configuration_follow_waiting")) {
		t.Fatalf("failed status read appeared as waiting for configuration: %q", tester.Texts())
	}
}
