package live

import (
	"context"
	"encoding/json"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// panelTabs drives the right panel of the whole desktop against a real Node,
// in Chinese and dark as the report was made: on an unsent draft and on a
// real session every kind of tab closes by its close button and by ⌘W, a
// closed tab stays closed across frames, navigation and a relaunch, the
// launcher lists what the conversation has, and no tab shows a blank body.
// It runs as a step of TestPanelProjection. SAILRY_SNAPSHOTS names a folder
// that takes the frames.
func panelTabs(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	// The window's browser, as SetWebViewWindow makes it, without a native
	// window: a tab has no page until an address is entered.
	r.browser = &browserState{available: true, tabs: map[string]*browserTab{}, serving: map[*Runtime]context.CancelFunc{r: func() {}},
		documents: map[string]*documentView{}, sources: map[string]protocol.ArtifactContent{}}
	r.App.Panel.BrowserActions = &panel.BrowserActions{View: r.browserView, Page: r.browserPage, Navigate: r.browserNavigate,
		Back: func(id string) { r.browserStep(id, -1) }, Forward: func(id string) { r.browserStep(id, 1) }, Reload: func(id string) { r.browserStep(id, 0) }}
	r.App.Mode = theme.Dark
	const width, height = 1280, 820
	// Opaque surfaces: the software renderer spends the test's time on the
	// translucent ones otherwise.
	options := ui.TesterOptions{Dark: true, Preferences: ui.Preferences{ReduceTransparency: true}}
	tester := ui.NewTesterWith(r.App.View, width, height, options)
	tester.SetLocale("zh-CN")
	text := func(key string) string { return i18n.In("zh-CN", key) }
	app := r.App
	settle := func() {
		t.Helper()
		for range 6 {
			r.workers.Wait()
			tester.Advance(200 * time.Millisecond)
			tester.Frame()
		}
	}
	wait := func(what string, done func() bool) {
		t.Helper()
		for end := time.Now().Add(20 * time.Second); time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
			settle()
			if done() {
				return
			}
		}
		t.Fatalf("timed out waiting for %s; tabs %v; visible text: %q", what, app.Panel.Tabs, tester.Texts())
	}
	shot := func(name string) {
		t.Helper()
		dir := os.Getenv("SAILRY_SNAPSHOTS")
		if dir == "" {
			return
		}
		file, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	}
	// left is the panel's left edge; body is the middle of the active tab's
	// body, under the header and any toolbar.
	left := func() float32 { return width - app.PanelWidth }
	inPanel := func(info ui.AccessInfo) bool { return info.Bounds.X >= left() }
	// row is a launcher row, a button of the panel 40 DIPs high.
	row := func(key string) (ui.AccessInfo, bool) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == text(key) && inPanel(info) && info.Bounds.H == 40 {
				return info, true
			}
		}
		return ui.AccessInfo{}, false
	}
	// strip is a tab of the strip, a button 28 DIPs high in the header's
	// segment over the panel.
	strip := func(title string) (ui.AccessInfo, bool) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == title && inPanel(info) && info.Bounds.H == 28 && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
				return info, true
			}
		}
		return ui.AccessInfo{}, false
	}
	// title is a tab's title in the strip.
	title := func(tab panel.Tab) string {
		switch {
		case tab.Kind == panel.Artifact:
			return "note.txt"
		case tab.Kind == panel.Subagent:
			return "Child"
		case tab.ID != "" && (tab.Kind == panel.Files || tab.Kind == panel.Changes):
			return filepath.Base(tab.ID)
		}
		return text(panel.ResourceOf(tab.Kind).Label)
	}
	has := func(tab panel.Tab) bool { return slices.Contains(app.Panel.Tabs, tab) }
	// shown reports whether the panel's body shows want.
	shown := func(want string) bool {
		for _, info := range tester.AccessAll() {
			if inPanel(info) && info.Bounds.Y > shell.HeaderHeight && strings.Contains(info.Name, want) {
				return true
			}
		}
		return false
	}
	// stays checks that a closed tab does not come back: over frames, in
	// the tabs a relaunch would restore and, when away is set, over the
	// settings page and back.
	stays := func(name string, tab panel.Tab, away bool) {
		t.Helper()
		settle()
		if has(tab) {
			t.Fatalf("%s: the closed tab came back after more frames: %v", name, app.Panel.Tabs)
		}
		if saved := app.Prefs(); slices.Contains(saved.PanelTabs, tab) || saved.PanelTab == tab {
			t.Fatalf("%s: the saved window state keeps the closed tab: %v, active %v", name, saved.PanelTabs, saved.PanelTab)
		}
		if !away {
			return
		}
		selected := app.Sidebar.Selected
		app.Navigate(shell.SettingsPage)
		settle()
		app.Navigate(shell.ConversationPage)
		if app.Data.Find(selected) != nil {
			app.Sidebar.Selected = selected
		}
		settle()
		if has(tab) {
			t.Fatalf("%s: the closed tab came back after navigating away and back: %v", name, app.Panel.Tabs)
		}
	}
	// closes opens a tab, checks that its body shows want and closes it by
	// its close button; then again by ⌘W with the focus in the panel. The
	// tab is the only one, so each close shows the launcher.
	closes := func(name string, want string, open func() panel.Tab) {
		t.Helper()
		for _, by := range []string{"close button", "⌘W"} {
			app.PanelOpen = true
			tab := open()
			settle()
			if !has(tab) || app.Panel.Active != tab {
				t.Fatalf("%s: the tab did not open: %v, active %v; visible text: %q", name, app.Panel.Tabs, app.Panel.Active, tester.Texts())
			}
			if want != "" && !shown(want) {
				t.Fatalf("%s: the tab's body does not show %q: %q", name, want, tester.Texts())
			}
			button, found := strip(title(tab))
			if !found {
				t.Fatalf("%s: no tab titled %q in the strip: %q", name, title(tab), tester.Texts())
			}
			if by == "⌘W" {
				// The focus goes into the panel as its tab is chosen.
				tester.ClickAt(button.Bounds.X+12, button.Bounds.Y+button.Bounds.H/2)
				settle()
				if !has(tab) {
					t.Fatalf("%s: choosing the tab closed it", name)
				}
				tester.Key(ui.Cmd, ui.KeyW)
			} else {
				// The close button is the 20 DIP box at the tab's end.
				x, y := button.Bounds.X+button.Bounds.W-14, button.Bounds.Y+button.Bounds.H/2
				tester.Move(x, y)
				tester.Frame()
				tester.ClickAt(x, y)
			}
			settle()
			if has(tab) || len(app.Panel.Tabs) != 0 {
				t.Fatalf("%s: the %s left the tab open: %v", name, by, app.Panel.Tabs)
			}
			// Every kind leaves and comes back once per conversation state.
			stays(name+" by "+by, tab, by == "close button")
			// The last tab closed: the panel is back at the launcher, which
			// ⌘W closes with it.
			app.PanelOpen = true
			settle()
			if _, listed := row("resource_browser"); !listed {
				t.Fatalf("%s: closing the last tab by %s did not return to the launcher: %q", name, by, tester.Texts())
			}
		}
	}
	// launcherLists checks the launcher's rows and the "+" menu against the
	// kinds the conversation offers.
	// offered is what a conversation lists: the reference's four kinds,
	// never History or Documents, and a terminal needs a worktree.
	offered := func(kind panel.Kind, session, terminal bool) bool {
		if kind == panel.Terminal {
			return terminal
		}
		return kind != panel.History && kind != panel.Documents
	}
	launcherLists := func(state string, session, terminal bool) {
		t.Helper()
		app.PanelOpen = true
		settle()
		for _, resource := range panel.Resources {
			want := offered(resource.Kind, session, terminal)
			if _, listed := row(resource.Label); listed != want {
				t.Fatalf("%s: the launcher lists %q: %v, want %v; visible text: %q", state, resource.Label, listed, want, tester.Texts())
			}
		}
		// The "+" menu shows over a tab.
		app.Panel.Show(panel.Tab{Kind: panel.Files})
		settle()
		add, found := ui.AccessInfo{}, false
		for _, info := range tester.AccessAll() {
			// The header's details toggle has the same name and is a plain button.
			if info.Role == ui.RoleMenuButton && info.Name == text("details") && inPanel(info) && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
				add, found = info, true
			}
		}
		if !found {
			t.Fatalf("%s: no + button in the strip: %q", state, tester.Texts())
		}
		tester.ClickAt(add.Bounds.X+add.Bounds.W/2, add.Bounds.Y+add.Bounds.H/2)
		settle()
		items := map[string]bool{}
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleMenuItem || info.Role == ui.RoleMenuItemCheckBox || info.Role == ui.RoleMenuItemRadio {
				items[info.Name] = true
			}
		}
		for _, resource := range panel.Resources {
			want := offered(resource.Kind, session, terminal)
			if items[text(resource.Label)] != want {
				t.Fatalf("%s: the + menu lists %q: %v, want %v; items %v", state, resource.Label, items[text(resource.Label)], want, items)
			}
		}
		tester.Key(0, ui.KeyEscape)
		settle()
		app.Panel.Close(panel.Tab{Kind: panel.Files})
		settle()
	}
	artifact := func() panel.Tab {
		app.Panel.ShowArtifact(conversation.File{ID: "note", Path: "note.txt", Kind: conversation.TextFile, Text: "panel artifact body"})
		return panel.Tab{Kind: panel.Artifact, ID: "note"}
	}
	child := func() panel.Tab {
		app.Panel.ShowChild("child", "Child")
		return panel.Tab{Kind: panel.Subagent, ID: "child"}
	}
	browser := func() panel.Tab {
		app.Panel.New(panel.Browser)
		return app.Panel.Active
	}
	show := func(tab panel.Tab) func() panel.Tab {
		return func() panel.Tab {
			app.Panel.Show(tab)
			return tab
		}
	}
	openKind := func(kind panel.Kind) func() panel.Tab {
		return func() panel.Tab {
			app.Panel.Open(kind)
			return app.Panel.Active
		}
	}

	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}

	// An unsent draft: no session and no project.
	draft := app.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") || len(controller.Snapshot().Sessions) != 0 {
		t.Fatalf("a new profile did not open on a draft: %q", draft)
	}
	tester.Key(ui.Cmd|ui.Alt, ui.KeyB)
	tester.Advance(time.Second)
	settle()
	if !app.PanelOpen || !app.Panel.Live || app.Panel.Session {
		t.Fatalf("the draft's panel: open %v, live %v, session %v", app.PanelOpen, app.Panel.Live, app.Panel.Session)
	}
	launcherLists("draft", false, false)
	shot("panel-launcher-draft")
	// The reported case: History opened on the draft, then its close button.
	closes("draft History", text("panel_history_empty"), show(panel.Tab{Kind: panel.History}))
	closes("draft Documents", text("panel_documents_empty"), show(panel.Tab{Kind: panel.Documents}))
	closes("draft Browser", text("browser_empty"), browser)
	closes("draft Files", text("resource_project_empty"), openKind(panel.Files))
	closes("draft Git", text("resource_project_empty"), openKind(panel.Changes))
	closes("draft Artifact", "Code editor", artifact)
	closes("draft Subagent", text("chat_child_unavailable"), child)
	// The reported failure: Terminal on a new chat without a project. The
	// chat has no worktree, the window refused the creation itself and
	// reported its fault as the generic "operation failed". Terminal is not
	// offered there now (launcherLists above): neither Open nor its
	// shortcut opens a tab, asks the Node or toasts.
	if app.Panel.Worktree || app.Panel.Offers(panel.Terminal) {
		t.Fatal("a draft without a project is offered a terminal")
	}
	app.Panel.Open(panel.Terminal)
	tester.Key(ui.Ctrl, ui.KeyBackquote)
	settle()
	if len(app.Panel.Tabs) != 0 || len(controller.Snapshot().Terminals) != 0 {
		t.Fatalf("a draft without a project opened a terminal: tabs %v, Node terminals %d", app.Panel.Tabs, len(controller.Snapshot().Terminals))
	}
	for _, key := range []string{"chat_action_failed", "terminal_unavailable"} {
		if tester.HasText(text(key)) {
			t.Fatalf("Terminal on a draft without a project toasts %q", text(key))
		}
	}
	// Asked for all the same, the creation names its reason.
	r.createPanelResource(panel.Terminal)
	settle()
	if !tester.HasText(text("terminal_unavailable")) || tester.HasText(text("chat_action_failed")) || len(app.Panel.Tabs) != 0 {
		t.Fatalf("a terminal that cannot start does not say why: %q", tester.Texts())
	}
	if err := tester.Click(text("close")); err != nil {
		t.Fatal(err)
	}
	settle()

	// A real session in a project with a changed file.
	folder, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	git := func(arguments ...string) {
		t.Helper()
		command := exec.Command("git", append([]string{"-C", folder, "-c", "user.name=Panel", "-c", "user.email=panel@example.invalid", "-c", "commit.gpgsign=false"}, arguments...)...)
		command.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_CONFIG_SYSTEM="+os.DevNull)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", arguments, err, output)
		}
	}
	if err := os.WriteFile(filepath.Join(folder, "README.md"), []byte("# Panel\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	git("init", "-q")
	git("add", "README.md")
	git("commit", "-q", "-m", "Add the readme")
	if err := os.WriteFile(filepath.Join(folder, "README.md"), []byte("# Panel\n\nchanged line\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	project, valid := detail[protocol.Project](do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Panel workspace", Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("the registered project was not returned")
	}
	// A provider no request is sent to: a session can be created on it.
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Panel fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data)
	if !valid {
		t.Fatal("the created session was not returned")
	}
	wait("the session in the sidebar", func() bool {
		r.OpenSession(session.ID.String())
		return app.Sidebar.Selected == session.ID.String() && r.workspace != nil && r.workspace.selected == session.Worktree && app.Panel.Session
	})
	wait("the worktree's files and changes", func() bool {
		pr := r.workspaceProject()
		return pr != nil && len(pr.Data.Changes) == 1 && pr.Data.Files != nil && len(pr.Data.Files.Children) > 0
	})
	// Reading the worktree opened no tab by itself.
	if len(app.Panel.Tabs) != 0 {
		t.Fatalf("selecting the session opened tabs: %v", app.Panel.Tabs)
	}
	launcherLists("session", true, true)
	shot("panel-launcher-session")

	// History and Documents open from the launcher and show their empty
	// state; each closes.
	for _, resource := range []struct {
		kind  panel.Kind
		empty string
		frame string
	}{{panel.History, "panel_history_empty", "panel-history-empty"}, {panel.Documents, "panel_documents_empty", "panel-documents-empty"}} {
		label := panel.ResourceOf(resource.kind).Label
		closes("session "+label, text(resource.empty), func() panel.Tab {
			app.PanelOpen = true
			settle()
			launch, listed := row(label)
			if !listed {
				t.Fatalf("the session's launcher does not list %q", label)
			}
			tester.ClickAt(launch.Bounds.X+launch.Bounds.W/2, launch.Bounds.Y+launch.Bounds.H/2)
			settle()
			wait("the empty "+label+" tab", func() bool { return shown(text(resource.empty)) })
			shot(resource.frame)
			return app.Panel.Active
		})
	}
	closes("session Browser", text("browser_empty"), browser)
	closes("session Files", text("file_empty"), openKind(panel.Files))
	closes("session file", "README.md", show(panel.Tab{Kind: panel.Files, ID: "README.md"}))
	closes("session Git", "", openKind(panel.Changes))
	closes("session diff", "README.md", show(panel.Tab{Kind: panel.Changes, ID: "README.md"}))
	closes("session Artifact", "Code editor", artifact)
	closes("session Subagent", text("chat_child_unavailable"), child)
	// A changed worktree reported again does not reopen the closed diff.
	if err := os.WriteFile(filepath.Join(folder, "README.md"), []byte("# Panel\n\nchanged again\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	r.workspace.projects[session.Worktree].refresh()
	settle()
	settle()
	if len(app.Panel.Tabs) != 0 {
		t.Fatalf("a Git refresh reopened a closed tab: %v", app.Panel.Tabs)
	}

	if runtime.GOOS == "windows" {
		t.Log("the terminal tabs are not exercised on Windows here; their ConPTY check is the integration program's")
	} else {
		panelConversations(t, panelFixture{
			r: r, controller: controller, tester: tester, settle: settle, wait: wait, shot: shot, text: text, row: row, strip: strip, do: do,
			session: session, config: config,
		})
	}

	// Several tabs: closing one that is not shown keeps the shown one, and
	// closing the shown one moves to the first left.
	first, second, third := panel.Tab{Kind: panel.History}, panel.Tab{Kind: panel.Documents}, panel.Tab{Kind: panel.Files, ID: "README.md"}
	for _, tab := range []panel.Tab{first, second, third} {
		app.Panel.Show(tab)
	}
	app.PanelOpen = true
	settle()
	button, found := strip(title(first))
	if !found {
		t.Fatalf("no History tab among three: %q", tester.Texts())
	}
	x, y := button.Bounds.X+button.Bounds.W-14, button.Bounds.Y+button.Bounds.H/2
	tester.Move(x, y)
	tester.Frame()
	tester.ClickAt(x, y)
	settle()
	if !slices.Equal(app.Panel.Tabs, []panel.Tab{second, third}) || app.Panel.Active != third {
		t.Fatalf("closing a tab behind the shown one: %v, active %v", app.Panel.Tabs, app.Panel.Active)
	}
	tester.Key(ui.Cmd, ui.KeyW)
	settle()
	if !slices.Equal(app.Panel.Tabs, []panel.Tab{second}) || app.Panel.Active != second || !app.PanelOpen {
		t.Fatalf("⌘W on the shown tab of two: %v, active %v, open %v", app.Panel.Tabs, app.Panel.Active, app.PanelOpen)
	}

	// A relaunch restores the open tab and none of the closed ones.
	dir := t.TempDir()
	if err := shell.SavePrefs(dir, app.Prefs()); err != nil {
		t.Fatal(err)
	}
	again, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer again.Close()
	again.App.SetPrefs(shell.LoadPrefs(dir))
	again.OpenSession(session.ID.String())
	relaunched := ui.NewTesterWith(again.App.View, width, height, options)
	relaunched.SetLocale("zh-CN")
	for range 8 {
		again.workers.Wait()
		relaunched.Advance(200 * time.Millisecond)
		relaunched.Frame()
	}
	if !slices.Equal(again.App.Panel.Tabs, []panel.Tab{second}) {
		t.Fatalf("the relaunched window's tabs: %v, want %v", again.App.Panel.Tabs, []panel.Tab{second})
	}
}

// panelFixture is panelTabs' window for panelConversations.
type panelFixture struct {
	r          *Runtime
	controller *client.Client
	tester     *ui.Tester
	settle     func()
	wait       func(string, func() bool)
	shot       func(string)
	text       func(string) string
	row, strip func(string) (ui.AccessInfo, bool)
	do         func(protocol.Command) protocol.Output
	session    protocol.Session
	config     protocol.SessionConfig
}

// panelConversations is panelTabs' part on terminal tabs and on the panel
// belonging to its conversation, against the real Node: Terminal opens a new
// Node terminal as a tab every time; each conversation has its own tabs,
// shown tab and column, which leave and come back with it while its terminals
// keep running; a draft that becomes its session keeps its tabs; closing a
// terminal's tab asks and then ends its shell; and a conversation that is
// removed or archived takes its terminals and tabs with it.
func panelConversations(t *testing.T, f panelFixture) {
	r, app, tester, settle, wait, text := f.r, f.r.App, f.tester, f.settle, f.wait, f.text
	first := f.session.ID.String()
	terminals := func(tabs []panel.Tab) []string {
		var ids []string
		for _, tab := range tabs {
			if tab.Kind == panel.Terminal {
				ids = append(ids, tab.ID)
			}
		}
		return ids
	}
	status := func(id string) string {
		for _, info := range f.controller.Snapshot().Terminals {
			if info.ID.String() == id {
				return info.Status.Kind
			}
		}
		return ""
	}
	running := func() int {
		count := 0
		for _, info := range f.controller.Snapshot().Terminals {
			if info.Status.Kind == "running" {
				count++
			}
		}
		return count
	}
	strips := func(title string) (found []ui.AccessInfo) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == title && info.Bounds.X >= 1280-app.PanelWidth && info.Bounds.H == 28 && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
				found = append(found, info)
			}
		}
		return found
	}
	// confirm answers the dialog that asks before a terminal's tab closes.
	confirm := func(answer string) {
		t.Helper()
		wait("the close confirmation", func() bool {
			return tester.HasText(text("workspace_close_terminal")) && tester.HasText(text("settings_cancel"))
		})
		var button ui.AccessInfo
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == text(answer) && info.Bounds.Y > shell.HeaderHeight && info.Bounds.Y >= button.Bounds.Y {
				button = info
			}
		}
		if button.Name == "" {
			t.Fatalf("the confirmation has no %q: %q", text(answer), tester.Texts())
		}
		tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		settle()
	}
	label := text(panel.ResourceOf(panel.Terminal).Label)

	// The first conversation, a session of a Git project: Terminal in the
	// launcher opens a terminal's tab, and Terminal in the "+" menu another.
	app.PanelOpen = true
	settle()
	launch, listed := f.row(panel.ResourceOf(panel.Terminal).Label)
	if !listed || len(app.Panel.Tabs) != 0 {
		t.Fatalf("the session's launcher lists no Terminal: tabs %v, text %q", app.Panel.Tabs, tester.Texts())
	}
	tester.ClickAt(launch.Bounds.X+launch.Bounds.W/2, launch.Bounds.Y+launch.Bounds.H/2)
	wait("the launcher's Terminal starts a Node terminal", func() bool {
		ids := terminals(app.Panel.Tabs)
		return len(ids) == 1 && status(ids[0]) == "running" && app.Panel.Active.ID == ids[0]
	})
	one := app.Panel.Active
	var add ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleMenuButton && info.Name == text("details") && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
			add = info
		}
	}
	if add.Name == "" {
		t.Fatalf("no + button over the terminal: %q", tester.Texts())
	}
	tester.ClickAt(add.Bounds.X+add.Bounds.W/2, add.Bounds.Y+add.Bounds.H/2)
	settle()
	item := ui.AccessInfo{}
	for _, info := range tester.AccessAll() {
		if (info.Role == ui.RoleMenuItem || info.Role == ui.RoleMenuItemCheckBox || info.Role == ui.RoleMenuItemRadio) && info.Name == label {
			item = info
		}
	}
	if item.Name == "" {
		t.Fatalf("the + menu lists no Terminal: %q", tester.Texts())
	}
	tester.ClickAt(item.Bounds.X+item.Bounds.W/2, item.Bounds.Y+item.Bounds.H/2)
	wait("the + menu's Terminal starts a second Node terminal", func() bool {
		ids := terminals(app.Panel.Tabs)
		return len(ids) == 2 && ids[0] == one.ID && status(ids[1]) == "running" && app.Panel.Active.ID == ids[1] && len(strips(label)) == 2
	})
	two := app.Panel.Active
	wait("both terminals attach", func() bool {
		return r.packages.terminals[one.ID] != nil && r.packages.terminals[one.ID].terminal != nil &&
			r.packages.terminals[two.ID] != nil && r.packages.terminals[two.ID].terminal != nil
	})
	f.shot("panel-terminals-two")
	firstTabs := slices.Clone(app.Panel.Tabs)

	// A new chat without a project is another conversation: it shows none of
	// the first one's tabs, its panel is closed, and the terminals keep
	// running attached.
	r.newDraft(nil)
	settle()
	draft := app.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") || len(app.Panel.Tabs) != 0 || app.PanelOpen || app.Panel.Offers(panel.Terminal) {
		t.Fatalf("a new chat shows the session's panel: %q, tabs %v, open %v", draft, app.Panel.Tabs, app.PanelOpen)
	}
	if !slices.Equal(app.ConversationTabs(first), firstTabs) || running() != 2 || r.packages.terminals[one.ID] == nil || r.packages.terminals[two.ID] == nil {
		t.Fatalf("the hidden conversation lost its terminals: tabs %v, running %d", app.ConversationTabs(first), running())
	}
	// Typed text keeps the unsent chat when another conversation shows.
	app.Composer(draft).Draft = "Keep my tabs"
	app.PanelOpen = true
	app.Panel.New(panel.Browser)
	settle()
	app.Panel.Show(panel.Tab{Kind: panel.Files})
	page := app.Panel.Tabs[0]
	settle()
	draftTabs := slices.Clone(app.Panel.Tabs)
	if len(draftTabs) != 2 || page.Kind != panel.Browser || slices.Contains(firstTabs, page) {
		t.Fatalf("the draft's tabs: %v", draftTabs)
	}
	f.shot("panel-second-conversation")
	// Back and forth: each conversation has its own tabs and shown tab.
	wait("the first conversation's tabs come back", func() bool {
		r.OpenSession(first)
		return app.Sidebar.Selected == first && slices.Equal(app.Panel.Tabs, firstTabs) && app.Panel.Active == two && app.PanelOpen && len(strips(label)) == 2
	})
	if !slices.Equal(app.ConversationTabs(draft), draftTabs) {
		t.Fatalf("the draft's tabs while the session shows: %v", app.ConversationTabs(draft))
	}
	// The window state names each session's tabs; an unsent chat's are not
	// kept, as its ID is the window's.
	if saved := app.Prefs(); saved.PanelOf != first || !slices.Equal(saved.PanelTabs, firstTabs) || len(saved.Panels) != 0 {
		t.Fatalf("the saved panels: of %q, tabs %v, others %v", saved.PanelOf, saved.PanelTabs, saved.Panels)
	}
	app.Sidebar.Selected = draft
	settle()
	if !slices.Equal(app.Panel.Tabs, draftTabs) || !app.PanelOpen {
		t.Fatalf("the draft's tabs did not come back: %v, open %v", app.Panel.Tabs, app.PanelOpen)
	}

	// The draft becomes its session: the tabs stay its own, the browser
	// page with them, and the session's private worktree takes a terminal.
	cv, comp := app.Data.Find(draft), app.Composer(draft)
	r.enqueue(func(c *ui.Context) {
		actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		actions.Send(composer.Message{Text: comp.Draft, Model: comp.Model, Mode: comp.Mode, Permission: comp.Permission})
	})
	wait("the draft becomes a session", func() bool {
		return !strings.HasPrefix(app.Sidebar.Selected, "draft/") && app.Panel.Session && app.Panel.Worktree
	})
	second := app.Sidebar.Selected
	if second == first || !slices.Equal(app.Panel.Tabs, draftTabs) || len(app.ConversationTabs(draft)) != 0 || !app.PanelOpen {
		t.Fatalf("the created session %q lost its draft's tabs: %v", second, app.Panel.Tabs)
	}
	if tab := r.browser.tabs[page.ID]; tab == nil || tab.conversation != second {
		t.Fatalf("the draft's browser page is not the session's: %+v", tab)
	}
	if !app.Panel.Offers(panel.Terminal) {
		t.Fatal("a session without a project is offered no terminal")
	}
	app.Panel.Open(panel.Terminal)
	wait("a terminal in the session's private worktree", func() bool {
		ids := terminals(app.Panel.Tabs)
		return len(ids) == 1 && status(ids[0]) == "running" && running() == 3
	})
	three := app.Panel.Active
	for _, info := range f.controller.Snapshot().Terminals {
		if info.ID.String() == three.ID && (info.Worktree == nil || *info.Worktree == f.session.Worktree) {
			t.Fatalf("the session's terminal is not in its own worktree: %+v", info)
		}
	}
	if tester.HasText(text("chat_action_failed")) && len(terminals(app.Panel.Tabs)) != 1 {
		t.Fatalf("the terminal failed: %q", tester.Texts())
	}

	// A draft in a project that is a plain folder has the project's
	// worktree: it is offered a terminal, and one starts there.
	plain, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	folder, valid := detail[protocol.Project](f.do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Plain folder", Path: plain, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("the plain folder's project was not returned")
	}
	wait("the plain folder in the sidebar", func() bool {
		for _, shown := range app.Data.Projects {
			if shown.ID == folder.ID.String() {
				r.newDraft(shown)
				return true
			}
		}
		return false
	})
	wait("a draft in the plain folder", func() bool {
		return strings.HasPrefix(app.Sidebar.Selected, "draft/") && app.Data.ProjectOf(app.Sidebar.Selected) != nil && app.Panel.Worktree && app.Panel.Offers(panel.Terminal)
	})
	folderDraft := app.Sidebar.Selected
	if len(app.Panel.Tabs) != 0 || app.PanelOpen {
		t.Fatalf("the project's draft shows another conversation's panel: %v", app.Panel.Tabs)
	}
	app.PanelOpen = true
	app.Panel.Open(panel.Terminal)
	wait("a terminal in the plain folder", func() bool {
		ids := terminals(app.Panel.Tabs)
		return len(ids) == 1 && status(ids[0]) == "running" && running() == 4
	})
	four := app.Panel.Active
	for _, info := range f.controller.Snapshot().Terminals {
		if info.ID.String() == four.ID && workspaceString(info.Directory) != plain {
			t.Fatalf("the draft's terminal runs in %q, want %q", workspaceString(info.Directory), plain)
		}
	}
	// The draft is left empty and goes: its terminal ends and its tabs go
	// with it. The other conversations' terminals are not touched.
	wait("a removed draft ends its terminal", func() bool {
		r.OpenSession(first)
		return app.Sidebar.Selected == first && app.Data.Find(folderDraft) == nil && status(four.ID) == "closed" && !app.HasPanelTab(four)
	})
	if running() != 3 || !slices.Equal(app.Panel.Tabs, firstTabs) || !slices.Equal(terminals(app.ConversationTabs(second)), []string{three.ID}) {
		t.Fatalf("removing a draft disturbed the others: running %d, tabs %v, second %v", running(), app.Panel.Tabs, app.ConversationTabs(second))
	}

	// The agent's browser calls address the tabs of its own session's panel,
	// shown or not. Two tabs opened in the shown session and left before a
	// frame showed them have no page yet: they stay that session's. The
	// session then shown lists its own page only, and the hidden session's
	// call without a tab goes to the tab its own panel shows.
	app.Panel.New(panel.Browser)
	app.Panel.New(panel.Browser)
	opened := slices.Clone(app.Panel.Tabs[len(firstTabs):])
	r.OpenSession(second)
	wait("the second session shows again", func() bool {
		return app.Sidebar.Selected == second && slices.Contains(app.Panel.Tabs, page) && !slices.Contains(app.Panel.Tabs, opened[0])
	})
	if len(opened) != 2 || !slices.Equal(app.ConversationTabs(first), append(slices.Clone(firstTabs), opened...)) || r.browser.tabs[opened[0].ID] != nil || r.browser.tabs[opened[1].ID] != nil {
		t.Fatalf("the hidden session's new tabs: %v in %v, pages %v %v", opened, app.ConversationTabs(first), r.browser.tabs[opened[0].ID], r.browser.tabs[opened[1].ID])
	}
	var answer struct {
		Tab  uint64
		Tabs []struct{ ID uint64 }
	}
	plan := r.planBrowserCall(r.controller.Target(), second, browserRequest{Action: "tabs"})
	if plan.result == nil || plan.result.Ok == nil || json.Unmarshal(*plan.result.Ok, &answer) != nil || len(answer.Tabs) != 1 || answer.Tabs[0].ID != r.browser.tabs[page.ID].id {
		t.Fatalf("the shown session's browser tabs: %+v", plan.result)
	}
	if r.browser.tabs[opened[0].ID] != nil || r.browser.tabs[opened[1].ID] != nil {
		t.Fatal("the shown session's call took the hidden session's tabs")
	}
	shownTab := app.Panel.Active
	focused := r.planBrowserCall(r.controller.Target(), first, browserRequest{Action: "focus"})
	target := r.browser.tabs[opened[1].ID]
	if focused.result == nil || focused.result.Ok == nil || json.Unmarshal(*focused.result.Ok, &answer) != nil || target == nil || answer.Tab != target.id {
		t.Fatalf("the hidden session's call without a tab: %+v, want the tab its panel shows, %v", focused.result, opened[1])
	}
	if other := r.browser.tabs[opened[0].ID]; other == nil || other.conversation != first || target.conversation != first {
		t.Fatalf("the hidden session's pages are not its own: %+v %+v", other, target)
	}
	if app.Panel.Active != shownTab || slices.Contains(app.Panel.Tabs, opened[1]) || app.ConversationActiveTab(first) != opened[1] {
		t.Fatalf("the hidden session's call changed the shown panel: %v, active %v", app.Panel.Tabs, app.Panel.Active)
	}
	wait("the first session's new tabs come back with it", func() bool {
		r.OpenSession(first)
		return app.Sidebar.Selected == first && slices.Equal(app.Panel.Tabs, append(slices.Clone(firstTabs), opened...)) && app.Panel.Active == opened[1]
	})

	// A page that asks for a window gets a tab of its own conversation's
	// panel instead: in front for a page in front, behind for a page the
	// agent keeps in the background, in the hidden panel for a hidden
	// conversation's page, and none for an address no tab opens.
	source := r.browser.tabs[opened[1].ID]
	browserTabOf := func(tab *browserTab) panel.Tab {
		return panel.Tab{Kind: panel.Browser, ID: strconv.FormatUint(tab.id, 10)}
	}
	for _, address := range []string{"file:///etc/passwd", "javascript:alert(1)", "about:blank", "https://user:secret@example.com/"} {
		if r.browserPopup(source, address, true) != nil {
			t.Fatalf("a page opened a tab at %q", address)
		}
	}
	front := r.browserPopup(source, "https://example.com/front", true)
	if front == nil || front.conversation != first || front.node != source.node || front.background || app.Panel.Active != browserTabOf(front) {
		t.Fatalf("a window asked for by the page in front: %+v, active %v", front, app.Panel.Active)
	}
	source.background = true
	behind := r.browserPopup(source, "https://example.com/behind", false)
	if behind == nil || !behind.background || behind.conversation != first || app.Panel.Active != browserTabOf(front) || !slices.Contains(app.Panel.Tabs, browserTabOf(behind)) {
		t.Fatalf("a window asked for by a background page: %+v, active %v, tabs %v", behind, app.Panel.Active, app.Panel.Tabs)
	}
	source.background = false
	hidden := r.browserPopup(r.browser.tabs[page.ID], "https://example.com/hidden", true)
	if hidden == nil || hidden.conversation != second || slices.Contains(app.Panel.Tabs, browserTabOf(hidden)) || !slices.Contains(app.ConversationTabs(second), browserTabOf(hidden)) || app.Panel.Active != browserTabOf(front) {
		t.Fatalf("a window asked for by a hidden conversation's page: %+v, shown tabs %v, its tabs %v", hidden, app.Panel.Tabs, app.ConversationTabs(second))
	}
	// Without a gesture the conversation's tabs are bounded; a user's click
	// still opens one.
	popups := []panel.Tab{browserTabOf(front), browserTabOf(behind)}
	for {
		added := r.browserPopup(source, "https://example.com/script", false)
		if added == nil {
			break
		}
		if popups = append(popups, browserTabOf(added)); len(popups) > browserPopupTabs {
			t.Fatalf("scripts opened %d tabs", len(popups))
		}
	}
	pages := 0
	for _, tab := range app.Panel.Tabs {
		if tab.Kind == panel.Browser {
			pages++
		}
	}
	clicked := r.browserPopup(source, "https://example.com/clicked", true)
	if pages != browserPopupTabs || clicked == nil {
		t.Fatalf("the conversation has %d browser tabs after its scripts' windows; a click opened %v", pages, clicked)
	}
	// A closed tab's page opens nothing.
	app.Panel.Close(browserTabOf(clicked))
	if r.browserPopup(clicked, "https://example.com/late", true) != nil {
		t.Fatal("the page of a closed tab opened a tab")
	}
	for _, tab := range popups {
		app.Panel.Close(tab)
	}
	app.DropPanelTab(browserTabOf(hidden))

	app.Panel.Show(two)
	for _, tab := range opened {
		app.Panel.Close(tab)
	}
	settle()
	if !slices.Equal(app.Panel.Tabs, firstTabs) || app.Panel.Active != two || r.browser.tabs[opened[0].ID] != nil || r.browser.tabs[opened[1].ID] != nil {
		t.Fatalf("closing the new tabs: %v, active %v", app.Panel.Tabs, app.Panel.Active)
	}

	// Closing a terminal's tab asks first. Cancel keeps the shell; Close ends
	// it on the Node, and only then does the tab leave. ⌘W asks the same.
	closeButton := func(index int) {
		t.Helper()
		var button ui.AccessInfo
		wait("the terminal tabs at rest", func() bool {
			found := strips(label)
			if len(found) <= index {
				return false
			}
			previous := button.Bounds.X
			button = found[index]
			return button.Bounds.X == previous
		})
		x, y := button.Bounds.X+button.Bounds.W-14, button.Bounds.Y+button.Bounds.H/2
		tester.Move(x, y)
		tester.Frame()
		tester.ClickAt(x, y)
	}
	closeButton(1)
	confirm("settings_cancel")
	if status(two.ID) != "running" || !slices.Equal(app.Panel.Tabs, firstTabs) || tester.HasText(text("settings_cancel")) {
		t.Fatalf("Cancel closed the terminal: %s, tabs %v", status(two.ID), app.Panel.Tabs)
	}
	closeButton(1)
	confirm("close")
	wait("the confirmed close ends the Node terminal", func() bool {
		return status(two.ID) == "closed" && slices.Equal(app.Panel.Tabs, []panel.Tab{one}) && r.packages.terminals[two.ID] == nil
	})
	if status(one.ID) != "running" || status(three.ID) != "running" {
		t.Fatalf("closing one terminal ended another: %s %s", status(one.ID), status(three.ID))
	}
	found := strips(label)
	if len(found) != 1 {
		t.Fatalf("the strip after the close: %q", tester.Texts())
	}
	tester.ClickAt(found[0].Bounds.X+12, found[0].Bounds.Y+found[0].Bounds.H/2)
	settle()
	tester.Key(ui.Cmd, ui.KeyW)
	confirm("close")
	wait("⌘W ends the last terminal", func() bool { return status(one.ID) == "closed" && len(app.Panel.Tabs) == 0 })

	// An archived session takes its terminal and its tabs with it.
	var current protocol.Session
	for _, candidate := range f.controller.Snapshot().Sessions {
		if candidate.ID.String() == second {
			current = candidate
		}
	}
	f.do(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: current.ID, ExpectedRevision: current.Revision, Archived: true}})
	wait("an archived session ends its terminal", func() bool {
		return status(three.ID) == "closed" && len(app.ConversationTabs(second)) == 0 && !slices.Contains(app.PanelConversations(), second)
	})
	if running() != 0 || len(app.AllPanelTabs()) != 0 || app.Sidebar.Selected != first {
		t.Fatalf("after the archive: running %d, tabs %v, selected %q", running(), app.AllPanelTabs(), app.Sidebar.Selected)
	}
	// The archived session's browser page left with its tab.
	settle()
	if r.browser.tabs[page.ID] != nil {
		t.Fatal("the archived session's browser page stayed")
	}
	// The closed panel left no toast of a failure.
	for _, key := range []string{"chat_action_failed", "workspace_action_failed", "terminal_unavailable"} {
		if tester.HasText(text(key)) {
			t.Fatalf("the terminal steps left the toast %q: %q", text(key), tester.Texts())
		}
	}
}
