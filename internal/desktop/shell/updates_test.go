package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// About's updater checks, downloads, asks before installing and stops at
// Restart; the experience channel's check fails with Details.
func TestUpdater(t *testing.T) {
	a, tt := newTester(t)
	a.Do("app.about")
	tt.Frame()
	tt.Frame()
	step := func(label string, wait time.Duration) {
		t.Helper()
		clickButton(t, tt, i18n.In("en", label))
		tt.Frame()
		for range int(wait / (500 * time.Millisecond)) {
			tt.Advance(500 * time.Millisecond)
			tt.Frame()
		}
	}
	u := &a.Settings.Updater
	step("updates_check", 2*time.Second)
	if u.State != settings.UpdateAvailable || !tt.HasText("0.2.0-preview") {
		t.Fatalf("check: %v %q", u.State, tt.Texts())
	}
	step("updates_download", 5*time.Second)
	if u.State != settings.UpdateReady {
		t.Fatalf("download: %v", u.State)
	}
	snapshot(t, tt, "updates-ready")
	step("updates_install", time.Second)
	if !tt.HasText(i18n.In("en", "updates_restart_description")) {
		t.Fatalf("no confirmation: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "updates_install"))
	tt.Frame()
	for range 4 {
		tt.Advance(500 * time.Millisecond)
		tt.Frame()
	}
	if u.State != settings.UpdateStopped || !tt.HasText(i18n.In("en", "updates_restart_needed")) {
		t.Fatalf("install: %v", u.State)
	}
	t.Run("live states", func(t *testing.T) { testLiveUpdater(t) })
	*u = settings.Updater{}
	a.Settings.Channel = 1
	tt.Frame()
	step("updates_check", 2*time.Second)
	if u.Failure == nil || !tt.HasText(i18n.In("en", "updates_details")) {
		t.Fatalf("no failure: %+v", u)
	}
}

// testLiveUpdater shows each state of the real updater on the About page and
// presses its action: the page only reports the state and calls the action
// (SABOUT-005, 006, 008, 009).
func testLiveUpdater(t *testing.T) {
	a, tt := newTester(t)
	calls := map[string]int{}
	count := func(name string) func() { return func() { calls[name]++ } }
	a.Settings.UpdateActions = &settings.UpdateActions{Check: count("check"), Download: count("download"), Cancel: count("cancel"), Install: count("install"), Restart: count("restart")}
	a.Do("app.about")
	tt.Frame()
	tt.Frame()
	u := &a.Settings.Updater
	show := func(state settings.Updater) {
		t.Helper()
		*u = state
		tt.Frame()
		tt.Frame()
	}
	disabled := func(label string) bool {
		t.Helper()
		info, found := tt.AccessByRole(ui.RoleButton, i18n.In("en", label))
		if !found {
			t.Fatalf("no %s button: %q", label, tt.Texts())
		}
		return info.Disabled
	}
	has := func(label string) bool {
		_, found := tt.AccessByRole(ui.RoleButton, i18n.In("en", label))
		return found
	}

	// Downloading: the bytes copied as a percentage, then Verifying; Cancel
	// is the action and the channel is locked.
	show(settings.Updater{State: settings.UpdateDownloading, Version: "0.3.0", Copied: 12 << 20, Total: 48 << 20})
	if !tt.HasText("25%") || tt.HasText(i18n.In("en", "updates_verifying")) || !u.Locked() || has("updates_install") {
		t.Fatalf("downloading: %q", tt.Texts())
	}
	show(settings.Updater{State: settings.UpdateDownloading, Version: "0.3.0", Copied: 48 << 20, Total: 48 << 20, Verifying: true})
	if !tt.HasText(i18n.In("en", "updates_verifying")) || tt.HasText("100%") {
		t.Fatalf("verifying: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "settings_cancel"))
	tt.Frame()
	if calls["cancel"] != 1 || len(calls) != 1 {
		t.Fatalf("Cancel called %v", calls)
	}

	// Ready: Install asks first; only the confirmation installs, once.
	show(settings.Updater{State: settings.UpdateReady, Version: "0.3.0"})
	if !tt.HasText("0.3.0") {
		t.Fatalf("ready: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "updates_install"))
	tt.Frame()
	tt.Frame()
	if !tt.HasText(i18n.In("en", "updates_restart_description")) || calls["install"] != 0 {
		t.Fatalf("Install asked nothing or installed already: %v %q", calls, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "updates_install"))
	tt.Frame()
	tt.Frame()
	if calls["install"] != 1 || len(calls) != 2 {
		t.Fatalf("the confirmation called %v", calls)
	}

	// Installing and restarting: a disabled Installing button, the state's
	// text, no other action and a locked channel.
	for state, text := range map[settings.UpdateState]string{settings.UpdateInstalling: "updates_installing", settings.UpdateRestarting: "updates_restarting"} {
		show(settings.Updater{State: state, Version: "0.3.0"})
		if !disabled("updates_installing") || !tt.HasText(i18n.In("en", text)) || !u.Locked() ||
			has("updates_check") || has("settings_cancel") || has("updates_install") || has("updates_restart") || has("updates_download") {
			t.Fatalf("state %d: %q", state, tt.Texts())
		}
		clickButton(t, tt, i18n.In("en", "updates_installing"))
		tt.Frame()
		if calls["install"] != 1 || len(calls) != 2 {
			t.Fatalf("the disabled Installing button called %v", calls)
		}
	}

	// Stopped: Restart is the only action.
	show(settings.Updater{State: settings.UpdateStopped, Version: "0.3.0"})
	if !tt.HasText(i18n.In("en", "updates_restart_needed")) || has("updates_check") || has("updates_installing") {
		t.Fatalf("stopped: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "updates_restart"))
	tt.Frame()
	if calls["restart"] != 1 || len(calls) != 3 {
		t.Fatalf("Restart called %v", calls)
	}
}
