package live

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// railPages drives the rail's Files and Git pages of the whole desktop
// against a real Node: without a project either page asks for one and New
// project opens the project editor; with one the pages list the worktree's
// files and changes, open them from the Node and keep the right panel
// closed. It runs as a step of TestPanelProjection.
func railPages(t *testing.T) {
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
	tester := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	text := func(key string) string { return i18n.In("en", key) }
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
		t.Fatalf("timed out waiting for %s; visible text: %q", what, tester.Texts())
	}
	// rail clicks a feature's button on the rail.
	rail := func(key string) {
		t.Helper()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == text(key) && info.Bounds.X+info.Bounds.W <= shell.RailWidth {
				tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no %q on the rail: %q", text(key), tester.Texts())
	}
	tree := func(name string) (ui.AccessInfo, bool) { return tester.AccessByRole(ui.RoleTreeItem, name) }
	choose := func(name string) {
		t.Helper()
		row, found := tree(name)
		if !found {
			t.Fatalf("no row %q: %q", name, tester.Texts())
		}
		tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		settle()
	}
	code := func(want string) bool {
		for _, info := range tester.AccessAll() {
			if strings.Contains(info.Value, want) {
				return true
			}
		}
		return false
	}

	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}

	// An unsent draft has no project: either page asks for one.
	if !strings.HasPrefix(app.Sidebar.Selected, "draft/") {
		t.Fatalf("a new profile did not open on a draft: %q", app.Sidebar.Selected)
	}
	for _, page := range []struct {
		feature string
		page    shell.Page
	}{{"files", shell.FilesPage}, {"git", shell.GitPage}} {
		rail(page.feature)
		if app.Page != page.page || !tester.HasText(text("resource_project_empty")) || tester.HasText(text("unavailable")) {
			t.Fatalf("the %s page without a project: page %d, %q", page.feature, app.Page, tester.Texts())
		}
	}
	if err := tester.Click(text("project_new")); err != nil {
		t.Fatalf("no New project on the empty page: %v; %q", err, tester.Texts())
	}
	settle()
	if !app.Sidebar.Editing() || !tester.HasText(text("project_name")) {
		t.Fatalf("New project did not open the project editor: %q", tester.Texts())
	}
	tester.Key(0, ui.KeyEscape)
	settle()
	if app.Sidebar.Editing() {
		t.Fatal("Escape left the project editor open")
	}

	// A session in a project with a changed and an untracked file.
	folder, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	git := func(arguments ...string) {
		t.Helper()
		command := exec.Command("git", append([]string{"-C", folder, "-c", "user.name=Rail", "-c", "user.email=rail@example.invalid", "-c", "commit.gpgsign=false"}, arguments...)...)
		command.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_CONFIG_SYSTEM="+os.DevNull)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", arguments, err, output)
		}
	}
	write := func(name, content string) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(folder, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("notes.txt", "first line\n")
	git("init", "-q")
	git("add", "notes.txt")
	git("commit", "-q", "-m", "Add the notes")
	write("notes.txt", "first line\nrail changed line\n")
	write("main.go", "package railpages\n")
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	project, valid := detail[protocol.Project](do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Rail workspace", Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("the registered project was not returned")
	}
	// A provider no request is sent to: a session can be created on it.
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Rail fixture", protocol.ConversationModelApiChatCompletions
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
		return app.Sidebar.Selected == session.ID.String() && r.workspace != nil && r.workspace.selected == session.Worktree
	})
	if app.PanelOpen || len(app.Panel.Tabs) != 0 {
		t.Fatalf("the panel before the pages: open %v, tabs %v", app.PanelOpen, app.Panel.Tabs)
	}

	// Files: the worktree's tree, a file read from the Node.
	rail("files")
	wait("the worktree's files on the Files page", func() bool {
		_, listed := tree("main.go")
		return app.Page == shell.FilesPage && listed
	})
	if tester.HasText(text("unavailable")) || tester.HasText(text("resource_project_empty")) {
		t.Fatalf("the Files page of a project shows no content: %q", tester.Texts())
	}
	choose("main.go")
	wait("the file's text on the Files page", func() bool { return code("package railpages") })
	pr := r.workspaceProject()
	if pr == nil || pr.Files.File() != "main.go" {
		t.Fatalf("the Files page did not show the chosen file: %v", pr)
	}
	// A search result opens on the page, not in the panel behind it.
	r.workspace.projects[session.Worktree].openFileAt("notes.txt", 2)
	wait("the search result on the Files page", func() bool { return code("rail changed line") })
	if pr.Files.File() != "notes.txt" || app.Page != shell.FilesPage || app.PanelOpen {
		t.Fatalf("the search result: file %q, page %d, panel %v", pr.Files.File(), app.Page, app.PanelOpen)
	}

	// Git: the worktree's changes, a diff read from the Node.
	rail("git")
	wait("the worktree's changes on the Git page", func() bool {
		_, listed := tree("notes.txt")
		return app.Page == shell.GitPage && listed
	})
	if tester.HasText(text("unavailable")) || tester.HasText(text("resource_project_empty")) {
		t.Fatalf("the Git page of a project shows no content: %q", tester.Texts())
	}
	choose("notes.txt")
	wait("the diff on the Git page", func() bool { return pr.Git.Change() == "notes.txt" && code("rail changed line") })
	if app.PanelOpen {
		t.Fatal("the pages opened the right panel")
	}

	// The conversation's panel lists what the pages opened.
	rail("shell_sessions")
	app.PanelOpen = true
	settle()
	for _, tab := range []panel.Tab{{Kind: panel.Files, ID: "main.go"}, {Kind: panel.Files, ID: "notes.txt"}, {Kind: panel.Changes, ID: "notes.txt"}} {
		found := false
		for _, open := range app.Panel.Tabs {
			found = found || open == tab
		}
		if !found {
			t.Fatalf("the panel does not list the page's %v: %v", tab, app.Panel.Tabs)
		}
	}
}
