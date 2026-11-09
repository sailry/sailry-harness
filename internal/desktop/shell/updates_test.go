package shell

import (
	"testing"
	"time"

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
	*u = settings.Updater{}
	a.Settings.Channel = 1
	tt.Frame()
	step("updates_check", 2*time.Second)
	if u.Failure == nil || !tt.HasText(i18n.In("en", "updates_details")) {
		t.Fatalf("no failure: %+v", u)
	}
}
