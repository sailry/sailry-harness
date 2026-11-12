//go:build integration

package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// systemProbe stands in for the system's permission calls of this process.
// It never asks the system: its answers are the statuses the test sets.
type systemProbe struct {
	mu       sync.Mutex
	status   map[permissionflow.Permission]permissionflow.Status
	failing  map[permissionflow.Permission]bool
	checks   map[permissionflow.Permission]int
	requests map[permissionflow.Permission]int
	settings map[permissionflow.Permission]int
}

func newSystemProbe() *systemProbe {
	return &systemProbe{status: map[permissionflow.Permission]permissionflow.Status{}, failing: map[permissionflow.Permission]bool{},
		checks: map[permissionflow.Permission]int{}, requests: map[permissionflow.Permission]int{}, settings: map[permissionflow.Permission]int{}}
}

func (p *systemProbe) set(permission permissionflow.Permission, status permissionflow.Status, failing bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.status[permission], p.failing[permission] = status, failing
}

// counts are the checks, requests and settings openings of a permission.
func (p *systemProbe) counts(permission permissionflow.Permission) [3]int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return [3]int{p.checks[permission], p.requests[permission], p.settings[permission]}
}

// install gives the window's permissions page this probe before the page
// first shows, so the system's own calls are never made.
func (p *systemProbe) install(w *terminalUI, goos string) {
	root := w.runtime.localRuntime()
	root.permissions.probe = permissionProbe{
		goos: goos,
		check: func(permission permissionflow.Permission) (permissionflow.Status, error) {
			p.mu.Lock()
			defer p.mu.Unlock()
			p.checks[permission]++
			if p.failing[permission] {
				return permissionflow.Unknown, errors.New("fixture check failure")
			}
			return p.status[permission], nil
		},
		request: func(_ context.Context, permission permissionflow.Permission) (permissionflow.Status, error) {
			p.mu.Lock()
			defer p.mu.Unlock()
			p.requests[permission]++
			// The stand-in for the system prompt: the user allows it.
			p.status[permission] = permissionflow.Granted
			return permissionflow.Granted, nil
		},
		settings: func(permission permissionflow.Permission) error {
			p.mu.Lock()
			defer p.mu.Unlock()
			p.settings[permission]++
			return nil
		},
	}
	root.permissions.actions = settings.PermissionActions{Check: root.checkPermissions, Request: root.requestPermission, Settings: root.openPermissionSettings, ShowApp: root.showPermissionApp}
}

var permissionStatuses = []string{"Authorized", "Unauthorized", "Access denied", "Not checked", "Partially authorized", "Unavailable on this device", "Allow access on the execution device", "Not needed"}

// permissionRow is the status text under a row's label, "" without the row.
func permissionRow(w *terminalUI, label string) string {
	var rows []ui.AccessInfo
	for _, entry := range w.tester.AccessAll() {
		// The settings navigation is left of the page.
		if entry.Role == ui.RoleText && entry.Name == label && entry.Bounds.X > 500 {
			rows = append(rows, entry)
		}
	}
	if len(rows) != 1 {
		return ""
	}
	status, nearest := "", float32(48)
	for _, entry := range w.tester.AccessAll() {
		if entry.Role != ui.RoleText || !slices.Contains(permissionStatuses, entry.Name) {
			continue
		}
		if distance := entry.Bounds.Y - rows[0].Bounds.Y; distance > 0 && distance < nearest && entry.Bounds.X < rows[0].Bounds.X+40 {
			status, nearest = entry.Name, distance
		}
	}
	return status
}

// permissionButtons are the buttons named name on the row labelled label,
// from left to right.
func permissionButtons(w *terminalUI, label, name string) []ui.AccessInfo {
	var found []ui.AccessInfo
	for _, row := range w.named(ui.RoleText, label) {
		if row.Bounds.X <= 500 {
			continue
		}
		_, y := center(row.Bounds)
		for _, candidate := range w.buttons(name) {
			if _, cy := center(candidate.Bounds); cy > y-24 && cy < y+24 {
				found = append(found, candidate)
			}
		}
	}
	slices.SortFunc(found, func(a, b ui.AccessInfo) int { return int(a.Bounds.X - b.Bounds.X) })
	return found
}

// TestSystemPermissionsUI is the acceptance of Settings › Permissions:
// SSYS-001 to SSYS-005.
//
// Substitutions and limits: this process's grants are an injected
// permissionProbe (check, request, settings) installed before the page
// first shows, so permissionflow never asks the system, no prompt is raised
// and no settings pane opens; the statuses are therefore the probe's, not
// this Mac's. The computer package's grants come from the stand-in helper
// of internal/node/testdata/computerhelper, built by the test and located
// through SAILRY_HELPERS_DIR. Show app is driven from the test binary,
// which is not inside an application bundle: the page answers with its
// "installed app" toast and /usr/bin/open is never run; the bundle path is
// asserted through permissionBundle. The remote-host status needs a package
// grant without a check handler on a paired device, which no bundled
// package declares: it is not driven here.
func TestSystemPermissionsUI(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("the page's grants are read on macOS only")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 240*time.Second)
	defer cancel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	helpers := filepath.Join(root, "helpers")
	helper := filepath.Join(helpers, "Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
	if err := os.MkdirAll(filepath.Dir(helper), 0o700); err != nil {
		t.Fatal(err)
	}
	if output, err := exec.CommandContext(ctx, "go", "build", "-o", helper, "github.com/sailry/sailry-harness/internal/node/testdata/computerhelper").CombinedOutput(); err != nil {
		t.Fatalf("building the stand-in helper: %v\n%s", err, output)
	}
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	grants := func(accessibility, screen string) {
		t.Helper()
		body, _ := json.Marshal(map[string]string{"accessibility": accessibility, "screen_recording": screen})
		if err := os.WriteFile(filepath.Join(filepath.Dir(helper), "state.json"), body, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	calls := func() []string {
		body, _ := os.ReadFile(filepath.Join(filepath.Dir(helper), "calls.log"))
		var lines []string
		for _, line := range strings.Split(strings.TrimSpace(string(body)), "\n") {
			if line != "" {
				lines = append(lines, line)
			}
		}
		return lines
	}

	profile := filepath.Join(root, "profile")
	desk := newSystemDesk(t, ctx, profile, node.Options{})
	provider, model := desk.provider("http://127.0.0.1:9/v1")
	session := desk.session(desk.project("Permission workspace"), provider, model, "Permission notes")
	computer := func() (protocol.PluginInfo, bool) {
		if !slices.ContainsFunc(desk.setup.Snapshot().Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == "computer" }) {
			return protocol.PluginInfo{}, false
		}
		return detail[protocol.PluginInfo](desk.do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "computer"}}).Data)
	}
	enable := func(enabled bool) {
		t.Helper()
		info, installed := computer()
		if !installed {
			if !enabled {
				return
			}
			desk.do(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "computer"}})
			info, _ = computer()
		}
		if info.Summary.Enabled != enabled {
			desk.do(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "computer", ExpectedRevision: info.Summary.Revision, Enabled: enabled}})
		}
	}
	enable(false)

	const disk, access, microphone, screen = "Full Disk Access", "Accessibility", "Microphone", "Screen recording"
	rows := func(w *terminalUI) string {
		return fmt.Sprintf("disk %q, accessibility %q, microphone %q, screen %q", permissionRow(w, disk), permissionRow(w, access), permissionRow(w, microphone), permissionRow(w, screen))
	}
	expect := func(w *terminalUI, label string, want map[string]string) {
		t.Helper()
		settle(w, label+" (now "+rows(w)+")", systemUIWait, func() bool {
			state := &w.runtime.localRuntime().permissions
			if state.checking || state.pending {
				return false
			}
			for row, status := range want {
				if permissionRow(w, row) != status {
					return false
				}
			}
			return true
		})
	}
	quiet := func(w *terminalUI) {
		t.Helper()
		settle(w, "the permission work ends", systemUIWait, func() bool {
			state := &w.runtime.localRuntime().permissions
			return !state.checking && !state.pending
		})
	}

	// Step 1: the rows and statuses of this process's grants.
	probe := newSystemProbe()
	probe.set(permissionflow.FullDiskAccess, permissionflow.Denied, false)
	probe.set(permissionflow.Accessibility, permissionflow.Granted, false)
	probe.set(permissionflow.Microphone, permissionflow.Denied, false)
	window := openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	probe.install(window, "darwin")
	showSection(window, settings.Permissions)
	expect(window, "the page's first check", map[string]string{disk: "Unauthorized", access: "Authorized", microphone: "Access denied", screen: ""})
	for _, permission := range []permissionflow.Permission{permissionflow.FullDiskAccess, permissionflow.Accessibility, permissionflow.Microphone} {
		if counts := probe.counts(permission); counts != [3]int{1, 0, 0} {
			t.Fatalf("opening the page made %v checks, requests and settings openings of %s", counts, permission)
		}
	}
	if counts := probe.counts(permissionflow.ScreenRecording); counts != [3]int{} {
		t.Fatalf("screen recording was probed without a package that needs it: %v", counts)
	}
	if shown(window, "System access") != 1 || shown(window, "If Sailry is missing, add it with + or drag the app into the list") != 1 {
		t.Fatalf("the page lacks its title or the Full Disk Access help; visible text: %q", window.tester.Texts())
	}
	t.Logf("step 1 passed: opening the page checked each of the application's three grants once and showed Full Disk Access Unauthorized (the system reports it denied until granted), Accessibility Authorized, Microphone Access denied, no Screen recording row, and the Full Disk Access help")

	// Step 2: Check reads every grant again.
	probe.set(permissionflow.Microphone, permissionflow.NotDetermined, false)
	probe.set(permissionflow.Accessibility, permissionflow.Unknown, true)
	press(window, "Check")
	expect(window, "Check re-reads", map[string]string{disk: "Unauthorized", access: "Not checked", microphone: "Unauthorized"})
	// A failed check of one of this process's grants is the row's Not
	// checked status and one failure toast for the whole Check.
	toast(window, "Permission check failed")
	if count := lingering[window]["Permission check failed"]; count != 0 {
		t.Fatalf("the failure toast stays: %d show", count)
	}
	for _, permission := range []permissionflow.Permission{permissionflow.FullDiskAccess, permissionflow.Accessibility, permissionflow.Microphone} {
		if counts := probe.counts(permission); counts != [3]int{2, 0, 0} {
			t.Fatalf("Check made %v checks, requests and settings openings of %s", counts, permission)
		}
	}
	probe.set(permissionflow.Accessibility, permissionflow.NotDetermined, false)
	press(window, "Check")
	expect(window, "Check re-reads again", map[string]string{disk: "Unauthorized", access: "Unauthorized", microphone: "Unauthorized"})
	// Undetermined and ungranted are ordinary states, never a failure.
	noToast(window, "Permission check failed")
	t.Logf("step 2 passed: Check read all three grants again: an undetermined microphone and accessibility show Unauthorized without a toast, and a failing check shows Not checked with the Permission check failed toast, which runs out")

	// Step 3: Request asks once and the row follows the grant.
	clickRow(window, microphone, "Request access")
	expect(window, "the granted microphone", map[string]string{microphone: "Authorized", access: "Unauthorized"})
	if counts := probe.counts(permissionflow.Microphone); counts[1] != 1 || counts[2] != 0 {
		t.Fatalf("Request made %v checks, requests and settings openings of the microphone", counts)
	}
	if counts := probe.counts(permissionflow.Accessibility); counts[1] != 0 || counts[2] != 0 {
		t.Fatalf("the microphone's Request touched accessibility: %v", counts)
	}
	if _, found := rowAction(window, microphone, "Request access"); found {
		t.Fatal("a granted row still offers Request access")
	}
	clickRow(window, access, "Request access")
	expect(window, "the granted accessibility", map[string]string{microphone: "Authorized", access: "Authorized"})
	if counts := probe.counts(permissionflow.Accessibility); counts[1] != 1 || counts[2] != 0 {
		t.Fatalf("Request made %v checks, requests and settings openings of accessibility", counts)
	}
	// A refused microphone can only be changed in the system's settings.
	probe.set(permissionflow.Microphone, permissionflow.Denied, false)
	press(window, "Check")
	expect(window, "the refused microphone", map[string]string{microphone: "Access denied"})
	// In place of Request access the row has one Open settings, which
	// opens the pane and does not ask; the settings shortcut is a granted
	// row's.
	var action []ui.AccessInfo
	settle(window, "the refused microphone's one Open settings", systemUIWait, func() bool {
		action = permissionButtons(window, microphone, "Open settings")
		return len(action) == 1
	})
	window.clickOn(action[0])
	quiet(window)
	if counts := probe.counts(permissionflow.Microphone); counts[1] != 1 || counts[2] != 1 {
		t.Fatalf("a refused microphone's action made %v checks, requests and settings openings", counts)
	}
	if found := permissionButtons(window, access, "Open settings"); len(found) != 1 {
		t.Fatalf("the granted accessibility row has %d settings shortcuts", len(found))
	}
	t.Logf("step 3 passed: Request access asked the injected system once for the microphone and once for accessibility and each row became Authorized; a refused microphone offers one Open settings instead of Request access, which opened its pane exactly once and asked nothing; a granted row has its one settings shortcut")

	// Step 4: Open settings of Full Disk Access, before and after the grant.
	before := probe.counts(permissionflow.FullDiskAccess)
	clickRow(window, disk, "Open settings")
	quiet(window)
	if counts := probe.counts(permissionflow.FullDiskAccess); counts[0] != before[0]+1 || counts[1] != 0 || counts[2] != before[2]+1 {
		t.Fatalf("Open settings of Full Disk Access went from %v to %v", before, counts)
	}
	probe.set(permissionflow.FullDiskAccess, permissionflow.Granted, false)
	press(window, "Check")
	expect(window, "the granted disk access", map[string]string{disk: "Authorized"})
	if shown(window, "If Sailry is missing, add it with + or drag the app into the list") != 0 || len(window.buttons("Show app")) != 0 {
		t.Fatal("the Full Disk Access help stays after the grant")
	}
	before = probe.counts(permissionflow.FullDiskAccess)
	clickRow(window, disk, "Open settings")
	quiet(window)
	if counts := probe.counts(permissionflow.FullDiskAccess); counts[1] != 0 || counts[2] != before[2]+1 {
		t.Fatalf("the granted row's settings shortcut went from %v to %v", before, counts)
	}
	t.Logf("step 4 passed: Open settings of an ungranted Full Disk Access checked it once more and opened its pane once; after the grant the row is Authorized, the help is gone and the row's settings shortcut opened the pane once more, never a request")

	// Step 5: Show app, from a binary outside an application bundle.
	probe.set(permissionflow.FullDiskAccess, permissionflow.Denied, false)
	press(window, "Check")
	expect(window, "the ungranted disk access", map[string]string{disk: "Unauthorized"})
	executable, _ := os.Executable()
	if bundle := permissionBundle(executable); bundle != "" {
		t.Fatalf("the test binary lies in the bundle %q; Show app would reveal it in Finder", bundle)
	}
	press(window, "Show app")
	settle(window, "the installed-app toast", systemUIWait, func() bool { return visible(window, "Use the installed Sailry app to grant access") })
	for input, want := range map[string]string{
		"/Applications/Sailry.app/Contents/MacOS/sailry-desktop":                                 "/Applications/Sailry.app",
		"/Users/x/Apps/Sailry Beta.app/Contents/MacOS/sailry-desktop":                            "/Users/x/Apps/Sailry Beta.app",
		"/Applications/Sailry.app/Contents/Helpers/Sailry Computer Use.app/Contents/MacOS/agent": "/Applications/Sailry.app/Contents/Helpers/Sailry Computer Use.app",
		"/usr/local/bin/sailry-desktop":                                                          "",
		filepath.Join(root, "Sailry.app", "Contents", "MacOS", "sailry-desktop"):                 filepath.Join(root, "Sailry.app"),
	} {
		if got := permissionBundle(input); got != want {
			t.Fatalf("the bundle of %q is %q, want %q", input, got, want)
		}
	}
	t.Logf("step 5 passed: the help's Show app, pressed in a binary outside a bundle, showed Use the installed Sailry app to grant access and revealed nothing; permissionBundle resolves an executable to its enclosing .app")
	window.close()

	// Step 6: a system without these grants.
	other := newSystemProbe()
	window = openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	other.install(window, "linux")
	showSection(window, settings.Permissions)
	expect(window, "the unavailable rows", map[string]string{disk: "Unavailable on this device", access: "Unavailable on this device", microphone: "Unavailable on this device"})
	press(window, "Check")
	// The rows draw Request access disabled; a click does nothing.
	for _, row := range []string{disk, access, microphone} {
		found := permissionButtons(window, row, "Request access")
		if len(found) != 1 || !found[0].Disabled {
			t.Fatalf("the unavailable row %s offers %+v", row, found)
		}
		window.clickOn(found[0])
	}
	for range 20 {
		window.frame()
	}
	for _, permission := range []permissionflow.Permission{permissionflow.FullDiskAccess, permissionflow.Accessibility, permissionflow.Microphone} {
		if counts := other.counts(permission); counts != [3]int{} {
			t.Fatalf("a system without the grants was probed for %s: %v", permission, counts)
		}
	}
	if len(window.buttons("Open settings")) != 0 || len(window.buttons("Show app")) != 0 {
		t.Fatalf("unavailable rows offer settings or the help; elements: %s", window.elements())
	}
	expect(window, "the rows stay unavailable", map[string]string{disk: "Unavailable on this device", access: "Unavailable on this device", microphone: "Unavailable on this device"})
	t.Logf("step 6 passed: with the probe reporting a system other than macOS every row shows Unavailable on this device with no settings action, no help and a disabled Request access; Check and a click on each probed, asked and opened nothing")
	window.close()

	// Step 7: the computer package's grants through the stand-in helper.
	grants("granted", "denied")
	enable(true)
	probe = newSystemProbe()
	probe.set(permissionflow.FullDiskAccess, permissionflow.Granted, false)
	probe.set(permissionflow.Accessibility, permissionflow.Granted, false)
	probe.set(permissionflow.Microphone, permissionflow.Granted, false)
	window = openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
	defer window.close()
	probe.install(window, "darwin")
	showSection(window, settings.Permissions)
	expect(window, "the helper's rows", map[string]string{disk: "Authorized", access: "Authorized", microphone: "Authorized", screen: "Unauthorized"})
	if lines := calls(); len(lines) == 0 || slices.ContainsFunc(lines, func(line string) bool { return line != "run permissions" }) {
		t.Fatalf("the page's check ran the helper as %v", lines)
	}
	if counts := probe.counts(permissionflow.ScreenRecording); counts != [3]int{} {
		t.Fatalf("the helper's screen recording was probed in this process: %v", counts)
	}
	grants("denied", "denied")
	press(window, "Check")
	expect(window, "the helper lost accessibility", map[string]string{access: "Partially authorized", screen: "Unauthorized"})
	read := len(calls())
	clickRow(window, screen, "Request access")
	expect(window, "the helper's granted screen recording", map[string]string{screen: "Authorized"})
	asked := calls()[read:]
	if count := len(slices.DeleteFunc(slices.Clone(asked), func(line string) bool { return line != "run permissions --request" })); count != 1 {
		t.Fatalf("the screen recording request ran the helper as %v", asked)
	}
	if counts := probe.counts(permissionflow.ScreenRecording); counts != [3]int{} {
		t.Fatalf("the helper's request reached this process's probe: %v", counts)
	}
	// The stand-in grants every refused permission on a request.
	expect(window, "the helper's accessibility", map[string]string{access: "Authorized", screen: "Authorized"})
	t.Logf("step 7 passed: with the computer package enabled the Screen recording row appeared from the helper's `permissions` output (Unauthorized), a helper without accessibility made the shared row Partially authorized, and Request access ran `permissions --request` exactly once (helper calls: %v), after which both rows are Authorized; this process's probe was never asked about the helper's grants", asked)
}
