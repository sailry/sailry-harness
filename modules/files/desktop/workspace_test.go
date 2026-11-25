package desktop_test

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
	fixtures "github.com/sailry/sailry-harness/sdk/desktop/testing"
	"github.com/sailry/sailry-harness/sdk/native/node"
)

func TestFilesWorkspaceReadsAndOpensSearchResult(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	host := node.Assembly{Modules: []node.Module{filesnode.Module()}}
	owner, err := node.StartWithOptions(ctx, profile, node.Options{Assembly: host})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	window, err := fixtures.Connect(ctx, owner, profile, shell.Assemble(files.Register))
	if err != nil {
		t.Fatal(err)
	}
	defer window.Close()
	if installed := window.Controller.Snapshot().Plugins; len(installed) != 0 {
		t.Fatalf("native Files page fixture unexpectedly installed plugins: %+v", installed)
	}
	app := window.App
	tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.SetScale(0.125)
	text := func(key string) string { return i18n.In("en", key) }
	settle := func() {
		t.Helper()
		for range 6 {
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

	// An unsent draft has no project: the page asks for one.
	if !strings.HasPrefix(app.Sidebar.Selected, "draft/") {
		t.Fatalf("a new profile did not open on a draft: %q", app.Sidebar.Selected)
	}
	rail("files")
	if app.Page != shell.FilesPage || !tester.HasText(text("resource_project_empty")) || tester.HasText(text("unavailable")) {
		t.Fatalf("the Files page without a project: page %q, %q", app.Page, tester.Texts())
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

	folder := t.TempDir()
	write := func(name, content string) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(folder, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("notes.txt", "first line\nsearch result line\n")
	write("main.go", "package railpages\n")
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := window.Controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	project, valid := do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Rail workspace", Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data.(protocol.Project)
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
	session, valid := do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	if !valid {
		t.Fatal("the created session was not returned")
	}
	wait("the session in the sidebar", func() bool {
		window.OpenSession(session.ID.String())
		workspace := app.Hooks.WorkspaceProject()
		return app.Sidebar.Selected == session.ID.String() && workspace != nil && workspace.ID == session.Worktree.String()
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
	pr := app.Hooks.WorkspaceProject()
	if pr == nil || pr.Files.File() != "main.go" {
		t.Fatalf("the Files page did not show the chosen file: %v", pr)
	}
	// A search result opens on the page, not in the panel behind it.
	pr.Actions.OpenFileAt("notes.txt", 2)
	wait("the search result on the Files page", func() bool { return code("search result line") })
	if pr.Files.File() != "notes.txt" || app.Page != shell.FilesPage || app.PanelOpen {
		t.Fatalf("the search result: file %q, page %q, panel %v", pr.Files.File(), app.Page, app.PanelOpen)
	}

	// The conversation's panel lists what the pages opened: notes.txt took
	// the passing tab of main.go.
	rail("shell_sessions")
	app.PanelOpen = true
	settle()
	for _, open := range app.Panel.Tabs {
		if open == (panel.Tab{Kind: panel.Files, ID: "notes.txt"}) {
			return
		}
	}
	t.Fatalf("the panel does not list the file opened on the page: %v", app.Panel.Tabs)
}
