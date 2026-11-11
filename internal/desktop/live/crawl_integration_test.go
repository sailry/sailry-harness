//go:build integration

package live

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The live crawl presses every control the live window shows and compares
// the window after the press with the window under the resting pointer: a
// control whose press changes nothing the user can see, sends no command,
// opens no address and leaves the clipboard alone is reported as dead. It
// goes two levels deep: the controls a press reveals (a menu's items, a
// dialog's buttons) are pressed too.
//
// Each scene is a subtest with a desk of its own, so one scene's failure
// leaves the others' findings. A whole crawl takes hours of one core; run
// it by scene, as -run 'TestLiveCrawl/settings' does, or with
// SAILRY_CRAWL_SCENE, a regular expression over the scenes' names.
// SAILRY_CRAWL_OUT names a file that receives every finding.

// crawlNetwork are the commands that would leave this machine. The crawl's
// transport refuses them at once, so their buttons answer with a failure.
var crawlNetwork = []string{
	"refresh_model_catalog", "search_plugin_catalog", "search_skill_catalog", "read_catalog_plugin", "read_catalog_plugin_info",
	"check_plugin_updates", "install_plugin", "install_plugin_source", "inspect_plugin_source", "install_registry_skill",
	"discover_registry_skill", "discover_models", "search_models", "resolve_model", "install_local_model", "register_local_model",
	"read_model_versions", "read_model_scores", "send_diagnostics", "begin_provider_login", "start_connector_login", "probe_network",
	"probe_connection", "check_ssh", "install_host", "install_mcp", "check_plugin_mcp", "export_backup", "upload_backup", "submit_turn",
	"clone",
}

// crawlUnpressed are the controls the crawl does not press, each with its
// reason. A name is a control's accessible name in English; scene is a
// regular expression over the scene's name.
var crawlUnpressed = []struct{ scene, name, reason string }{
	{"settings/settings_dictation", "Request", "asks macOS for the microphone: the system's own prompt on this machine"},
	{"settings/settings_(dictation|knowledge|ocr)", "Install", "downloads a model of hundreds of megabytes on the controller"},
}

// crawlUnchanged are the controls whose press legitimately changes nothing,
// each with its reason. path is a regular expression over the finding's
// "scene: control" or "scene: control > control". A settings scene's own
// row in the settings list is allowed by crawlAllowed.
var crawlUnchanged = []struct{ path, reason string }{
	{`^page/market: button "All" #1$`, "the category already chosen"},
	{`^settings/settings_appearance: button "Default" #1$`, "the theme already applied"},
	{`^settings/settings_shortcuts: button ".*" #\d+ > button "Press shortcut" #1$`, "a shortcut being recorded waits for keys, not for a second press"},
	{`^(page|panel)/git: menu button "Git actions" #1 > menu item "(View options|Commit|Changes|Pull and push|Remotes|Stashes|Tags)" #1$`, "a submenu's row: the pointer over it has opened the submenu"},
}

// crawlAllowed is crawlUnchanged and, for every settings scene, the row of
// the settings list that names the page already shown.
func crawlAllowed() []struct{ path, reason string } {
	allowed := slices.Clone(crawlUnchanged)
	here := func(scene, label string) {
		allowed = append(allowed, struct{ path, reason string }{
			"^" + regexp.QuoteMeta(fmt.Sprintf("%s: button %q #1", scene, label)) + "$", "the settings page already shown",
		})
	}
	for _, section := range crawlSections {
		here("settings/"+string(section), i18n.In("en", string(section)))
	}
	for _, label := range crawlPluginPages {
		here("settings/plugin/"+label, label)
	}
	return allowed
}

var crawlSections = []settings.Section{settings.General, settings.Appearance, settings.Conversation, settings.Shortcuts, settings.Dictation,
	settings.Providers, settings.Connectors, settings.Teams, settings.Plugins, settings.Skills, settings.Knowledge, settings.Decision,
	settings.Terminal, settings.Connections, settings.Hooks, settings.Runtime, settings.OCR, settings.Permissions, settings.Backup, settings.About}

// crawlPluginPages are the settings pages the bundled plugins contribute, by
// their label.
var crawlPluginPages = []string{"Media models", "Memory", "Browser", "External Browser"}

type crawlTransport struct {
	link.Transport
	mu    *sync.Mutex
	kinds *[]string
}

func (t crawlTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	t.mu.Lock()
	*t.kinds = append(*t.kinds, request.Command.Kind)
	t.mu.Unlock()
	if request.Plugin == nil && slices.Contains(crawlNetwork, request.Command.Kind) {
		return link.Admission{}, errors.New("the crawl sends nothing off this machine")
	}
	return t.Transport.Dispatch(ctx, request)
}

// crawlDesk is a window over a real Node on a new profile with one project
// (a Git repository with a commit and a change), a provider and a session.
type crawlDesk struct {
	t       *testing.T
	runtime *Runtime
	tester  *ui.Tester
	session protocol.Session
	mu      sync.Mutex
	kinds   []string
	close   func()
}

func newCrawlDesk(t *testing.T, ctx context.Context) *crawlDesk {
	t.Helper()
	profile := filepath.Join(t.TempDir(), "profile")
	desk := newSystemDesk(t, ctx, profile, node.Options{})
	project := desk.project("Crawl")
	folder := project.Path
	for _, step := range [][]string{
		{"init", "-q"}, {"config", "user.email", "crawl@example.invalid"}, {"config", "user.name", "Crawl"},
	} {
		if out, err := exec.CommandContext(ctx, "git", append([]string{"-C", folder}, step...)...).CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", step, err, out)
		}
	}
	for name, text := range map[string]string{"README.md": "# Crawl\n", "main.go": "package main\n\nfunc main() {}\n"} {
		if err := os.WriteFile(filepath.Join(folder, name), []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	for _, step := range [][]string{{"add", "-A"}, {"commit", "-q", "-m", "Start"}} {
		if out, err := exec.CommandContext(ctx, "git", append([]string{"-C", folder}, step...)...).CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", step, err, out)
		}
	}
	if err := os.WriteFile(filepath.Join(folder, "README.md"), []byte("# Crawl\n\nChanged.\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	// No request reaches this endpoint: submit_turn is refused.
	provider, model := desk.provider("http://127.0.0.1:9/v1")
	session := desk.session(project, provider, model, "Crawl session")

	d := &crawlDesk{t: t, session: session}
	controller := client.New(crawlTransport{Transport: link.NewLocal(desk.owner.ID(), desk.owner.ID(), desk.owner), mu: &d.mu, kinds: &d.kinds})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		_ = controller.Close()
		t.Fatal(err)
	}
	d.close = func() {
		_ = r.Close()
		_ = controller.Close()
		_ = desk.setup.Close()
		_ = desk.owner.Stop(context.Background())
	}
	// The system's permission calls are never made: every grant reads as
	// not granted, a request grants it and the system's settings open nowhere.
	root := r.localRuntime()
	granted := map[permissionflow.Permission]bool{}
	var grants sync.Mutex
	root.permissions.probe = permissionProbe{
		goos: "darwin",
		check: func(permission permissionflow.Permission) (permissionflow.Status, error) {
			grants.Lock()
			defer grants.Unlock()
			if granted[permission] {
				return permissionflow.Granted, nil
			}
			return permissionflow.Denied, nil
		},
		request: func(_ context.Context, permission permissionflow.Permission) (permissionflow.Status, error) {
			grants.Lock()
			defer grants.Unlock()
			granted[permission] = true
			return permissionflow.Granted, nil
		},
		settings: func(permissionflow.Permission) error { return nil },
	}
	root.permissions.actions = settings.PermissionActions{Check: root.checkPermissions, Request: root.requestPermission, Settings: root.openPermissionSettings, ShowApp: root.showPermissionApp}
	r.App.NavOpen = true
	r.SetUpdate(func(func()) {})
	d.runtime = r
	d.tester = ui.NewTester(r.App.View, 1440, 1200)
	d.tester.SetLocale("en")
	d.settle()
	if d.tester.HasText(i18n.In("en", "close")) {
		_ = d.tester.Click(i18n.In("en", "close"))
		d.settle()
	}
	return d
}

// settle lets the window's work end and its animations run out.
func (d *crawlDesk) settle() {
	for range 2 {
		d.runtime.workers.Wait()
		d.tester.Frame()
		d.tester.Advance(400 * time.Millisecond)
		time.Sleep(10 * time.Millisecond)
	}
	d.runtime.workers.Wait()
	d.tester.Frame()
}

// signature is what a press may change: the picture, the texts, the
// commands sent, the addresses opened and the clipboard.
func (d *crawlDesk) signature() string {
	d.mu.Lock()
	sent := len(d.kinds)
	d.mu.Unlock()
	hash := sha256.New()
	hash.Write(d.tester.Image().Pix)
	fmt.Fprintf(hash, "\x00%q\x00%d\x00%q\x00%q", d.tester.Texts(), sent, d.tester.OpenedURLs(), d.tester.Clipboard())
	return string(hash.Sum(nil))
}

// crawlControl is a control by its role, name and place among its like.
type crawlControl struct {
	id     string
	name   string
	x, y   float32
	chosen bool // already the chosen one of its group
}

var crawlRoles = map[ui.Role]string{ui.RoleButton: "button", ui.RoleLink: "link", ui.RoleCheckBox: "checkbox", ui.RoleRadio: "radio",
	ui.RoleSwitch: "switch", ui.RolePopUpButton: "popup button", ui.RoleTab: "tab", ui.RoleMenuButton: "menu button",
	ui.RoleToggleButton: "toggle", ui.RoleComboBox: "combo box", ui.RoleDisclosure: "disclosure", ui.RoleTreeItem: "tree item",
	ui.RoleMenuItem: "menu item", ui.RoleMenuItemCheckBox: "menu checkbox", ui.RoleMenuItemRadio: "menu radio", ui.RoleOption: "option"}

// controls are the controls a press can reach. A radio button, a menu's
// radio item, a tab or an option that is already the chosen one is marked
// chosen and not pressed: choosing it again changes nothing, and its
// siblings are pressed.
func (d *crawlDesk) controls() []crawlControl {
	seen := map[string]int{}
	var out []crawlControl
	for _, x := range d.tester.AccessAll() {
		role, pressed := crawlRoles[x.Role]
		if !pressed || x.Disabled || x.Bounds.W < 2 || x.Bounds.H < 2 {
			continue
		}
		cx, cy := x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2
		if cx < 0 || cy < 0 || cx >= 1440 || cy >= 1200 {
			continue
		}
		key := fmt.Sprintf("%s %q", role, x.Name)
		seen[key]++
		chosen := false
		switch x.Role {
		case ui.RoleRadio, ui.RoleMenuItemRadio:
			chosen = x.Checked
		case ui.RoleTab, ui.RoleOption:
			chosen = x.Selected
		}
		out = append(out, crawlControl{id: fmt.Sprintf("%s #%d", key, seen[key]), name: x.Name, x: cx, y: cy, chosen: chosen})
	}
	return out
}

func (d *crawlDesk) control(id string) (crawlControl, bool) {
	for _, x := range d.controls() {
		if x.id == id {
			return x, true
		}
	}
	return crawlControl{}, false
}

// overlays counts the dialogs and popups shown.
func (d *crawlDesk) overlays() (n int) {
	for _, x := range d.tester.AccessAll() {
		if x.Role == ui.RoleDialog || x.Role == ui.RolePopup {
			n++
		}
	}
	return n
}

// press compares the window under the resting pointer with the window after
// the press, and reports whether the press changed it.
func (d *crawlDesk) press(x crawlControl) bool {
	d.tester.Move(x.x, x.y)
	d.settle()
	before := d.signature()
	d.tester.ClickAt(x.x, x.y)
	d.settle()
	return d.signature() != before
}

type crawlScene struct {
	name  string
	enter func(d *crawlDesk)
}

func crawlScenes() []crawlScene {
	conversation := func(d *crawlDesk) {
		d.runtime.App.Navigate(shell.ConversationPage)
		d.runtime.OpenSession(d.session.ID.String())
	}
	scenes := []crawlScene{
		{"page/draft", func(d *crawlDesk) {
			d.runtime.App.Navigate(shell.ConversationPage)
			d.runtime.App.PanelOpen = false
			d.runtime.newDraft(nil)
		}},
		{"page/conversation", func(d *crawlDesk) { conversation(d); d.runtime.App.PanelOpen = false }},
		{"page/project", func(d *crawlDesk) {
			conversation(d)
			d.settle()
			if project := d.runtime.App.Data.ProjectOf(d.session.ID.String()); project != nil {
				d.runtime.openProjectOverview(project)
			}
		}},
	}
	for _, page := range []struct {
		name string
		page shell.Page
	}{{"files", shell.FilesPage}, {"git", shell.GitPage}, {"activity", shell.ActivityPage}, {"market", shell.MarketPage},
		{"host", shell.HostPage}, {"notifications", shell.NotificationsPage}} {
		scenes = append(scenes, crawlScene{"page/" + page.name, func(d *crawlDesk) {
			conversation(d)
			d.settle()
			d.runtime.App.Navigate(page.page)
		}})
	}
	for _, resource := range panel.Resources {
		scenes = append(scenes, crawlScene{"panel/" + string(resource.Kind), func(d *crawlDesk) {
			conversation(d)
			d.settle()
			d.runtime.App.PanelOpen = true
			d.runtime.App.Panel.Open(resource.Kind)
		}})
	}
	for _, section := range crawlSections {
		scenes = append(scenes, crawlScene{"settings/" + string(section), func(d *crawlDesk) {
			d.runtime.App.Navigate(shell.SettingsPage)
			d.runtime.App.Settings.Open(section)
		}})
	}
	for _, label := range crawlPluginPages {
		scenes = append(scenes, crawlScene{"settings/plugin/" + label, func(d *crawlDesk) {
			d.runtime.App.Navigate(shell.SettingsPage)
			if d.tester.HasText(label) && !strings.HasPrefix(string(d.runtime.App.Settings.Section), "plugin:") {
				d.runtime.App.Settings.Open(settings.General)
				d.settle()
				var entry ui.AccessInfo
				for _, x := range d.tester.AccessAll() {
					if x.Role == ui.RoleButton && x.Name == label {
						entry = x
					}
				}
				d.tester.ClickAt(entry.Bounds.X+entry.Bounds.W/2, entry.Bounds.Y+entry.Bounds.H/2)
			}
		}})
	}
	return scenes
}

// TestLiveCrawl is the regression crawl of the live window: every page,
// panel kind and settings section, two levels deep.
func TestLiveCrawl(t *testing.T) {
	var filter *regexp.Regexp
	if pattern := os.Getenv("SAILRY_CRAWL_SCENE"); pattern != "" {
		filter = regexp.MustCompile(pattern)
	}
	allowed := crawlAllowed()
	var unexpected, notes []string
	// The report is written as each scene ends: a crawl that is stopped
	// keeps the scenes it finished.
	report := func() {
		path := os.Getenv("SAILRY_CRAWL_OUT")
		if path == "" {
			return
		}
		text := "DEAD\n" + strings.Join(unexpected, "\n") + "\n\nNOTES\n" + strings.Join(notes, "\n") + "\n"
		if err := os.WriteFile(path, []byte(text), 0o644); err != nil {
			t.Error(err)
		}
	}
	for _, scene := range crawlScenes() {
		if filter != nil && !filter.MatchString(scene.name) {
			continue
		}
		t.Run(scene.name, func(t *testing.T) {
			// A scene that fails on the way still reports what it found.
			var dead, skipped []string
			defer func() {
				sort.Strings(dead)
				for _, finding := range dead {
					index := slices.IndexFunc(allowed, func(entry struct{ path, reason string }) bool {
						return regexp.MustCompile(entry.path).MatchString(finding)
					})
					if index < 0 {
						unexpected = append(unexpected, finding)
						t.Errorf("a press changed nothing: %s", finding)
						continue
					}
					skipped = append(skipped, "allowed: "+finding+": "+allowed[index].reason)
				}
				for _, note := range skipped {
					t.Log(note)
				}
				notes = append(notes, skipped...)
				report()
			}()
			scene.crawl(t, t.Context(), &dead, &skipped)
		})
	}
}

// crawl presses the scene's controls and what each reveals. It appends the
// dead controls to dead and what it did not press to notes as it goes, so a
// scene that fails on the way keeps both.
func (scene crawlScene) crawl(t *testing.T, ctx context.Context, deadOut, notesOut *[]string) {
	var d *crawlDesk
	var base []crawlControl
	baseOverlays := 0
	note := func(format string, args ...any) { *notesOut = append(*notesOut, fmt.Sprintf(format, args...)) }
	dead := func(path string) {
		*deadOut = append(*deadOut, scene.name+": "+path)
		t.Logf("dead: %s: %s", scene.name, path)
	}
	open := func() {
		if d != nil {
			// Not closed twice when the new desk fails to start.
			old := d
			d = nil
			old.close()
		}
		d = newCrawlDesk(t, ctx)
		scene.enter(d)
		d.settle()
	}
	// reset brings the scene back; a dialog that will not close costs a new
	// desk.
	reset := func() {
		d.tester.CloseMenu()
		for range 2 {
			d.tester.Key(0, ui.KeyEscape)
			d.tester.Frame()
		}
		d.tester.Move(1, 1)
		d.runtime.App.NavOpen = true
		scene.enter(d)
		d.settle()
		if d.overlays() > baseOverlays {
			open()
		}
	}
	unpressed := func(name string) (string, bool) {
		for _, skip := range crawlUnpressed {
			if skip.name == name && regexp.MustCompile(skip.scene).MatchString(scene.name) {
				return skip.reason, true
			}
		}
		return "", false
	}
	defer func() {
		if d != nil {
			d.close()
		}
	}()
	open()
	base = d.controls()
	baseOverlays = d.overlays()
	known := map[string]bool{}
	for _, x := range base {
		known[x.id] = true
	}
	t.Logf("%s: %d controls", scene.name, len(base))
	for _, target := range base {
		reset()
		x, found := d.control(target.id)
		if !found {
			// An earlier press changed what the scene shows: a new desk.
			open()
			x, found = d.control(target.id)
		}
		if !found {
			note("gone: %s: %s (not shown on a new desk)", scene.name, target.id)
			continue
		}
		if x.chosen {
			continue
		}
		if reason, skip := unpressed(x.name); skip {
			note("unpressed: %s: %s: %s", scene.name, x.id, reason)
			continue
		}
		page, section := d.runtime.App.Page, d.runtime.App.Settings.Section
		if !d.press(x) {
			dead(x.id)
			continue
		}
		if d.runtime.App.Page != page || d.runtime.App.Settings.Section != section {
			continue // it navigated: the other scene covers what shows
		}
		var revealed []string
		for _, child := range d.controls() {
			if !known[child.id] {
				revealed = append(revealed, child.id)
			}
		}
		if len(revealed) > 12 {
			note("capped: %s: %s revealed %d controls, 12 pressed", scene.name, x.id, len(revealed))
			revealed = revealed[:12]
		}
		for _, id := range revealed {
			reset()
			parent, found := d.control(target.id)
			if !found {
				open()
				if parent, found = d.control(target.id); !found {
					break
				}
			}
			d.tester.ClickAt(parent.x, parent.y)
			d.settle()
			child, found := d.control(id)
			if !found || child.chosen {
				continue
			}
			if reason, skip := unpressed(child.name); skip {
				note("unpressed: %s: %s > %s: %s", scene.name, x.id, child.id, reason)
				continue
			}
			if !d.press(child) {
				dead(x.id + " > " + child.id)
			}
		}
	}
}
