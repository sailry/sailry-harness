//go:build integration

package live

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
)

// topButton is the highest of the visible buttons named name: a toast's
// action, above the page's button of the same name.
func topButton(w *terminalUI, name string, count int) (ui.AccessInfo, bool) {
	found := w.buttons(name)
	if len(found) != count {
		return ui.AccessInfo{}, false
	}
	return found[0], true
}

// TestSystemAboutUI is the B13 acceptance of Settings › About against the
// real update service: SABOUT-001 to SABOUT-007, SABOUT-010 and SABOUT-011.
//
// Substitutions: the release source is updater.FixtureFeed, a loopback feed
// signed with a generated ed25519 test key that reproduces the fixture of
// the updater's own feed integration test; the service's installed
// application is an unsigned fixture bundle under t.TempDir(), its relaunch
// only records, and the system's code-signature check is replaced because
// the fixture bundles are unsigned. Step 8 accepts the confirmation, so
// SABOUT-006, SABOUT-008 and SABOUT-009 run as far as the fixture goes: the
// fixture bundle is replaced and the relaunch is recorded. Installing over a
// signed application and starting it again remain a manual acceptance.
func TestSystemAboutUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 240*time.Second)
	defer cancel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	desk := newSystemDesk(t, ctx, profile, node.Options{})
	provider, model := desk.provider("http://127.0.0.1:9/v1")
	session := desk.session(desk.project("About workspace"), provider, model, "About notes")

	feed := updater.NewFixtureFeed()
	t.Cleanup(feed.Server.Close)
	const older, stable, experience, damaged = "0.0.9", "9.1.0", "9.2.0-beta.1", "9.3.0-beta.2"
	feed.Publish(older, false)
	installed := filepath.Join(root, "Applications", "Sailry.app")
	if err := updater.WriteFixtureBundle(installed, link.ProductVersion); err != nil {
		t.Fatal(err)
	}

	window := openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	defer window.close()
	// The service stops the Node, relaunches and quits on its own
	// goroutine; halt, when set, is what stopping the Node does.
	var effects sync.Mutex
	var stops, quits int
	var launched []string
	var halt func() error
	stop := func() error {
		effects.Lock()
		stops++
		held := halt
		effects.Unlock()
		if held != nil {
			return held()
		}
		return nil
	}
	quit := func() {
		effects.Lock()
		quits++
		effects.Unlock()
	}
	relaunched := func(executable string) {
		effects.Lock()
		launched = append(launched, executable)
		effects.Unlock()
	}
	// done reports the Node stops, the quits and the relaunches so far.
	done := func() (int, int, []string) {
		effects.Lock()
		defer effects.Unlock()
		return stops, quits, slices.Clone(launched)
	}
	updates := NewUpdates(window.runtime.Preferences, profile, stop, quit)
	t.Cleanup(feed.Bind(updates.Service, installed, relaunched))
	t.Cleanup(updates.Service.Close)
	window.runtime.SetUpdates(updates, func() bool { return true })
	showSection(window, settings.About)
	u := &window.runtime.App.Settings.Updater
	untouched := func(label string) {
		t.Helper()
		version, err := updater.FixtureBundleVersion(installed)
		if stops, quits, launched := done(); err != nil || version != link.ProductVersion || stops != 0 || quits != 0 || len(launched) != 0 {
			t.Fatalf("%s: the installed bundle is %q (%v), %d Node stops, %d quits, relaunched %v", label, version, err, stops, quits, launched)
		}
	}
	await := func(label string, reached <-chan struct{}) {
		t.Helper()
		select {
		case <-reached:
		case <-time.After(systemUIWait):
			t.Fatalf("%s: the feed was not asked", label)
		}
	}
	channel := func() ui.AccessInfo { return window.one(ui.RoleMenuButton, "Update channel") }
	saved := func() string {
		if value := preferences.Open(filepath.Join(profile, "desktop", "preferences.json")).Data.UpdateChannel; value != nil {
			return *value
		}
		return ""
	}

	// (1) SABOUT-001: the running build.
	settle(window, "the About page", systemUIWait, func() bool { return shown(window, "Check for updates") > 0 })
	for text, count := range map[string]int{"Version": 2, link.ProductVersion: 2, "Build type": 1, "Development": 1, "License": 1, "Apache-2.0": 1, "Update channel": 1} {
		if shown(window, text) < count {
			t.Fatalf("the About page shows %q %d times, want %d; visible text: %q", text, shown(window, text), count, window.tester.Texts())
		}
	}
	if feed.Hits("/releases") != 0 || u.State != settings.UpdateIdle {
		t.Fatalf("opening the page asked the feed %d times, state %v", feed.Hits("/releases"), u.State)
	}
	t.Logf("step 1 passed: About shows version %s, the Development build type and the license; opening it asks no feed", link.ProductVersion)

	// (2) SABOUT-002, SABOUT-003, SABOUT-011: Check, Cancel while checking,
	// and the up-to-date outcome.
	reached, release := feed.Stall("/releases")
	press(window, "Check for updates")
	await("Check", reached)
	settle(window, "the Checking status", systemUIWait, func() bool {
		return u.State == settings.UpdateChecking && shown(window, "Checking") == 1 && len(window.buttons("Cancel")) == 1 && len(window.buttons("Check for updates")) == 0 && channel().Disabled
	})
	press(window, "Cancel")
	settle(window, "a cancelled check returns to idle", systemUIWait, func() bool {
		return u.State == settings.UpdateIdle && shown(window, "Checking") == 0 && len(window.buttons("Check for updates")) == 1 && !channel().Disabled
	})
	release()
	for range 30 {
		window.frame()
	}
	if u.Failure != nil || len(window.buttons("Details")) != 0 || shown(window, "Sailry is up to date") != 0 {
		t.Fatalf("a cancelled check reported %+v; visible text: %q", u.Failure, window.tester.Texts())
	}
	press(window, "Check for updates")
	toast(window, "Sailry is up to date")
	if hits := feed.Hits("/releases"); hits != 2 || u.State != settings.UpdateIdle || shown(window, link.ProductVersion) < 2 {
		t.Fatalf("after an up-to-date check: %d feed requests, state %v", hits, u.State)
	}
	t.Logf("step 2 passed: Check showed Checking with Cancel and a locked channel; Cancel returned to idle without a toast; with only %s published the next Check toasted Sailry is up to date", older)

	// (3) SABOUT-002, SABOUT-004: an available version.
	feed.Publish(stable, false)
	feed.Publish(experience, false)
	press(window, "Check for updates")
	settle(window, "the available stable version", systemUIWait, func() bool {
		return u.State == settings.UpdateAvailable && shown(window, stable) == 1 && shown(window, experience) == 0 && len(window.buttons("Download")) == 1 && len(window.buttons("Check for updates")) == 1
	})
	t.Logf("step 3 passed: with %s and %s published the Stable check offers %s with Download", stable, experience, stable)

	// (4) SABOUT-011: the channel re-checks and persists.
	hits := feed.Hits("/releases")
	pickOption(window, "Update channel", "Experience")
	settle(window, "the Experience channel's version", systemUIWait, func() bool {
		return u.State == settings.UpdateAvailable && shown(window, experience) == 1 && feed.Hits("/releases") == hits+1 && saved() == "experience"
	})
	pickOption(window, "Update channel", "Stable")
	settle(window, "the Stable channel's version", systemUIWait, func() bool {
		return u.State == settings.UpdateAvailable && shown(window, stable) == 1 && feed.Hits("/releases") == hits+2 && saved() != "experience"
	})
	t.Logf("step 4 passed: choosing Experience checked again and offered %s, saved update_channel in preferences.json; Stable checked again and offered %s", experience, stable)

	// (5) SABOUT-003, SABOUT-005: progress and Cancel while downloading.
	reached, release = feed.Stall(feed.ArchivePath(stable))
	press(window, "Download")
	await("Download", reached)
	percent := ""
	settle(window, "the download's progress", systemUIWait, func() bool {
		if u.State != settings.UpdateDownloading || u.Copied <= 0 || u.Copied >= u.Total {
			return false
		}
		percent = fmt.Sprintf("%.0f%%", u.Percent())
		return shown(window, percent) == 1 && len(window.buttons("Cancel")) == 1 && channel().Disabled
	})
	copied, total := u.Copied, u.Total
	press(window, "Cancel")
	settle(window, "a cancelled download returns to the available version", systemUIWait, func() bool {
		return u.State == settings.UpdateAvailable && shown(window, stable) == 1 && shown(window, percent) == 0 && len(window.buttons("Download")) == 1
	})
	release()
	for range 30 {
		window.frame()
	}
	if u.Failure != nil || len(window.buttons("Details")) != 0 {
		t.Fatalf("a cancelled download reported %+v", u.Failure)
	}
	untouched("after a cancelled download")
	t.Logf("step 5 passed: with the archive held at %d of %d bytes the page showed %s with Cancel and a locked channel; Cancel returned to the available version without a toast", copied, total, percent)

	// (6) SABOUT-004, SABOUT-006, SABOUT-007: the ready toast and the
	// install confirmation, which the test cancels.
	ready := stable + " is ready to install"
	press(window, "Download")
	var action ui.AccessInfo
	settle(window, "the ready toast with Install", systemUIWait, func() bool {
		found, both := topButton(window, "Install and restart", 2)
		action = found
		return u.State == settings.UpdateReady && shown(window, ready) == 1 && both
	})
	if hits := feed.Hits(feed.ArchivePath(stable)); hits != 2 {
		t.Fatalf("the archive was requested %d times over a cancelled and a completed download", hits)
	}
	const consequence = "Drafts will be restored after restart; local tasks and terminals will stop"
	window.clickOn(action)
	settle(window, "the toast's Install opens the confirmation", systemUIWait, func() bool { return shown(window, consequence) == 1 })
	untouched("with the confirmation open")
	press(window, "Cancel")
	settle(window, "the cancelled confirmation closes", systemUIWait, func() bool { return shown(window, consequence) == 0 })
	// The toast went with its action; no second one follows.
	for range 60 {
		window.frame()
	}
	if shown(window, ready) != 0 || u.State != settings.UpdateReady || shown(window, stable) != 1 {
		t.Fatalf("after the ready toast ran out: %d toasts, state %v", shown(window, ready), u.State)
	}
	press(window, "Install and restart")
	settle(window, "the page's Install opens the confirmation", systemUIWait, func() bool { return shown(window, consequence) == 1 })
	press(window, "Cancel")
	settle(window, "the cancelled confirmation closes", systemUIWait, func() bool { return shown(window, consequence) == 0 })
	untouched("after two cancelled confirmations")
	t.Logf("step 6 passed: the completed download showed %q once with Install and restart; the toast's action and the page's button each opened the confirmation naming the consequence; both were cancelled and nothing was installed", ready)

	// (7) SABOUT-010: a failure keeps its toast, its Details and the
	// installed version.
	feed.Publish(damaged, true)
	pickOption(window, "Update channel", "Experience")
	settle(window, "the damaged release is offered", systemUIWait, func() bool {
		return u.State == settings.UpdateAvailable && shown(window, damaged) == 1 && len(window.buttons("Download")) == 1
	})
	press(window, "Download")
	settle(window, "the failed download", systemUIWait, func() bool {
		found, both := topButton(window, "Details", 2)
		action = found
		return u.Failure != nil && u.State != settings.UpdateDownloading && both
	})
	if u.Failure.Key != "updates_digest_invalid" || shown(window, "The update package is damaged") != 1 || u.State == settings.UpdateReady {
		t.Fatalf("the damaged archive failed as %+v in state %v; visible text: %q", u.Failure, u.State, window.tester.Texts())
	}
	for range 48 {
		window.tester.Advance(250 * time.Millisecond)
		window.tester.Frame()
	}
	if shown(window, "The update package is damaged") != 1 {
		t.Fatal("the failure toast ran out")
	}
	detail := u.Failure.Detail
	action, _ = topButton(window, "Details", 2)
	window.clickOn(action)
	settle(window, "the toast's Details opens the failure's detail", systemUIWait, func() bool { return detail != "" && visible(window, detail) })
	window.tester.Key(0, ui.KeyEscape)
	settle(window, "the details close", systemUIWait, func() bool { return !visible(window, detail) })
	press(window, "Details")
	settle(window, "the page's Details opens the failure's detail", systemUIWait, func() bool { return visible(window, detail) })
	window.tester.Key(0, ui.KeyEscape)
	settle(window, "the details close again", systemUIWait, func() bool { return !visible(window, detail) })
	untouched("after a failed download")
	t.Logf("step 7 passed: the damaged archive of %s failed with the persistent toast The update package is damaged; the toast's Details and the page's Details show %q; the installed bundle is still %s", damaged, detail, link.ProductVersion)

	// (8) SABOUT-006, SABOUT-008, SABOUT-009 as far as a fixture bundle
	// goes: the confirmation installs; while it runs the page shows the
	// disabled Installing button; an installation that stopped the Node but
	// could not finish offers Restart, which relaunches.
	busy := func(status string) func() bool {
		return func() bool {
			found := window.buttons("Installing")
			return len(found) == 1 && found[0].Disabled && shown(window, status) >= 1 && channel().Disabled &&
				len(window.buttons("Check for updates"))+len(window.buttons("Cancel"))+len(window.buttons("Download"))+len(window.buttons("Install and restart"))+len(window.buttons("Restart")) == 0
		}
	}
	stage := func(service string) {
		t.Helper()
		settle(window, "the stable version is offered to "+service, systemUIWait, func() bool {
			return u.State == settings.UpdateAvailable && u.Version == stable && len(window.buttons("Download")) == 1
		})
		press(window, "Download")
		toast(window, ready)
		settle(window, "the staged stable version of "+service, systemUIWait, func() bool {
			return u.State == settings.UpdateReady && len(window.buttons("Install and restart")) == 1
		})
		press(window, "Install and restart")
		settle(window, "the confirmation of "+service, systemUIWait, func() bool { return shown(window, consequence) == 1 })
	}
	pickOption(window, "Update channel", "Stable")
	stage("the first service")
	untouched("before the confirmation is accepted")
	entered, proceed := make(chan struct{}, 1), make(chan error)
	effects.Lock()
	halt = func() error { entered <- struct{}{}; return <-proceed }
	effects.Unlock()
	footerAction(window, "Cancel", "Install and restart")
	await("the installation stops the Node", entered)
	settle(window, "the Installing state", systemUIWait, func() bool { return u.State == settings.UpdateInstalling && busy("Installing")() })
	// The disabled button installs nothing more.
	window.clickOn(window.buttons("Installing")[0])
	for range 10 {
		window.frame()
	}
	if stops, quits, launched := done(); stops != 1 || quits != 0 || len(launched) != 0 || u.State != settings.UpdateInstalling {
		t.Fatalf("while installing: %d Node stops, %d quits, relaunched %v, state %v", stops, quits, launched, u.State)
	}
	proceed <- fmt.Errorf("the fixture Node did not stop")
	settle(window, "the stopped installation offers Restart", systemUIWait, func() bool {
		return u.State == settings.UpdateStopped && shown(window, "Restart required") == 1 && len(window.buttons("Restart")) == 1 && len(window.buttons("Installing")) == 0 &&
			shown(window, "Could not stop the local Node; restart Sailry") == 1 && channel().Disabled
	})
	if version, err := updater.FixtureBundleVersion(installed); err != nil || version != link.ProductVersion {
		t.Fatalf("an installation whose Node did not stop left the bundle at %q (%v)", version, err)
	}
	press(window, "Restart")
	await("Restart stops the Node", entered)
	settle(window, "the Restarting state", systemUIWait, func() bool { return u.State == settings.UpdateRestarting && busy("Restarting")() })
	proceed <- nil
	settle(window, "Restart relaunches and quits", systemUIWait, func() bool {
		stops, quits, launched := done()
		return stops == 2 && quits == 1 && len(launched) == 1
	})
	_, _, launches := done()
	if !strings.HasPrefix(launches[0], installed+string(filepath.Separator)) || u.State != settings.UpdateRestarting || !busy("Restarting")() {
		t.Fatalf("Restart relaunched %v and left the page in state %v", launches, u.State)
	}
	t.Logf("step 8a passed: the confirmed installation showed the disabled Installing button while the Node stopped; the Node's refusal left %s installed, toasted Could not stop the local Node and offered Restart, which showed Restarting, relaunched %s and quit once", link.ProductVersion, launches[0])

	// A new process's service installs for real into the fixture bundle.
	effects.Lock()
	halt = nil
	effects.Unlock()
	second := NewUpdates(window.runtime.Preferences, profile, stop, quit)
	t.Cleanup(feed.Bind(second.Service, installed, relaunched))
	t.Cleanup(second.Service.Close)
	window.runtime.SetUpdates(second, func() bool { return true })
	settle(window, "the new service is idle", systemUIWait, func() bool { return u.State == settings.UpdateIdle && len(window.buttons("Check for updates")) == 1 })
	press(window, "Check for updates")
	stage("the second service")
	footerAction(window, "Cancel", "Install and restart")
	settle(window, "the installation replaces the bundle, relaunches and quits", systemUIWait, func() bool {
		stops, quits, launched := done()
		return stops == 3 && quits == 2 && len(launched) == 2
	})
	version, err := updater.FixtureBundleVersion(installed)
	if _, _, launches = done(); err != nil || version != stable || launches[1] != launches[0] {
		t.Fatalf("the installation left the bundle at %q (%v) and relaunched %v", version, err, launches)
	}
	if _, err := os.Stat(filepath.Join(profile, "desktop", "updater", "receipt.json")); err != nil {
		t.Fatalf("the installation left no receipt: %v", err)
	}
	// The quitting process keeps the disabled Installing button to its end.
	for range 30 {
		window.frame()
	}
	if u.State != settings.UpdateInstalling || !busy("Installing")() {
		t.Fatalf("after the installation the page is in state %v; visible text: %q", u.State, window.tester.Texts())
	}
	t.Logf("step 8b passed: a second service staged %s again; its confirmation stopped the Node once, replaced the fixture bundle (now %s), wrote receipt.json, relaunched %s and quit, the page staying on the disabled Installing button", stable, version, launches[1])
}
