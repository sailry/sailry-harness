package live

import (
	"context"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
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
	// body, under the strip and any toolbar.
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
	// strip is a tab of the strip, a button of the panel 28 DIPs high in
	// the bar under the header.
	strip := func(title string) (ui.AccessInfo, bool) {
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == title && inPanel(info) && info.Bounds.H == 28 && info.Bounds.Y < shell.HeaderHeight+40 {
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
			if inPanel(info) && info.Bounds.Y > shell.HeaderHeight+40 && strings.Contains(info.Name, want) {
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
	launcherLists := func(state string, session bool) {
		t.Helper()
		app.PanelOpen = true
		settle()
		for _, resource := range panel.Resources {
			want := session || resource.Kind != panel.History && resource.Kind != panel.Documents
			if _, listed := row(resource.Label); listed != want {
				t.Fatalf("%s: the launcher lists %q: %v, want %v; visible text: %q", state, resource.Label, listed, want, tester.Texts())
			}
		}
		// The "+" menu shows over a tab.
		app.Panel.Show(panel.Tab{Kind: panel.Files})
		settle()
		add, found := ui.AccessInfo{}, false
		for _, info := range tester.AccessAll() {
			if info.Name == text("details") && inPanel(info) && info.Bounds.Y > shell.HeaderHeight && info.Bounds.Y < shell.HeaderHeight+40 {
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
			want := session || resource.Kind != panel.History && resource.Kind != panel.Documents
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
	launcherLists("draft", false)
	shot("panel-launcher-draft")
	// The reported case: History opened on the draft, then its close button.
	closes("draft History", text("panel_history_empty"), show(panel.Tab{Kind: panel.History}))
	closes("draft Documents", text("panel_documents_empty"), show(panel.Tab{Kind: panel.Documents}))
	closes("draft Browser", text("browser_address"), browser)
	closes("draft Files", text("resource_project_empty"), openKind(panel.Files))
	closes("draft Git", text("resource_project_empty"), openKind(panel.Changes))
	closes("draft Artifact", "Code editor", artifact)
	closes("draft Subagent", text("chat_child_unavailable"), child)
	// A terminal needs a worktree: the draft opens no tab for one and says
	// so in a toast.
	app.Panel.Open(panel.Terminal)
	settle()
	if len(app.Panel.Tabs) != 0 {
		t.Fatalf("a draft without a project opened a terminal tab: %v", app.Panel.Tabs)
	}
	if err := tester.Click(text("close")); err != nil {
		t.Fatalf("no toast for the terminal a draft cannot open: %v; visible text: %q", err, tester.Texts())
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
	launcherLists("session", true)
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
	closes("session Browser", text("browser_address"), browser)
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

	// A terminal tab is a Node terminal: closing the tab detaches it, and
	// the running terminal does not bring the tab back.
	if runtime.GOOS == "windows" {
		t.Log("the terminal tab is not exercised on Windows here; its ConPTY check is the integration program's")
	} else {
		closes("session Terminal", "", func() panel.Tab {
			before := len(controller.Snapshot().Terminals)
			app.Panel.Open(panel.Terminal)
			wait("a Node terminal for the tab", func() bool {
				return len(controller.Snapshot().Terminals) == before+1 && app.Panel.Active.Kind == panel.Terminal
			})
			return app.Panel.Active
		})
		for _, terminal := range controller.Snapshot().Terminals {
			if terminal.Status.Kind != "closed" {
				do(protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: terminal.ID}})
			}
		}
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
