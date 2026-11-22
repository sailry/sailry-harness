package live

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesPage "github.com/sailry/sailry-harness/modules/files/desktop"
)

type workspaceDirectoryRead struct {
	input protocol.CommandListDirectoryPayload
	done  chan link.Response
}

type workspaceDirectoryTransport struct {
	resourcePageTransport
	files *conversationTestSubscription
	reads chan workspaceDirectoryRead
}

func (transport *workspaceDirectoryTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "files" {
		return transport.files, nil
	}
	return transport.resourcePageTransport.Subscribe(ctx, topic)
}

func (transport *workspaceDirectoryTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if input, ok := detail[protocol.CommandListDirectoryPayload](request.Command.Data); request.Command.Kind == "list_directory" && ok {
		done := make(chan link.Response, 1)
		select {
		case transport.reads <- workspaceDirectoryRead{input, done}:
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
		case <-ctx.Done():
			return link.Admission{}, ctx.Err()
		}
	}
	return transport.resourcePageTransport.Dispatch(ctx, request)
}

func TestDirectoryWatchPreservesFileTree(t *testing.T) {
	for _, surface := range []string{"files-module", "conversation-sidebar"} {
		t.Run(surface, func(t *testing.T) {
			testDirectoryWatchPreservesFileTree(t, surface)
		})
	}
}

func testDirectoryWatchPreservesFileTree(t *testing.T, surface string) {
	snapshot := protocol.DefaultSnapshot()
	node, project, worktree, session := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.SessionID{4}
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Listing fixture", Path: "/fixture"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture", Main: true}}
	snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree, Revision: 1,
		Config: protocol.DefaultSessionConfig(), Roles: protocol.DefaultRoleSnapshot()}}
	chat := protocol.DefaultConversationSnapshot()
	chat.Node, chat.Page.Session, chat.Page.Revision = node, session, 1
	transport := &workspaceDirectoryTransport{
		resourcePageTransport: resourcePageTransport{conversationTestTransport{snapshot: snapshot, conversation: chat}},
		files:                 &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
		reads:                 make(chan workspaceDirectoryRead, 12),
	}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		var output protocol.Output
		switch request.Command.Kind {
		case "inspect_git":
			output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}
		case "list_artifacts":
			output = protocol.Output{Kind: "artifacts", Data: protocol.ArtifactPage{}}
		}
		return link.Response{Output: output}, output.Kind != ""
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	r, err := New(t.Context(), controller, t.TempDir(), filesPage.Page())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = r.Close() })
	app := r.App
	app.SetWelcome(true)
	app.Effects = false
	app.Sidebar.Selected = session.String()
	if surface == "files-module" {
		app.Navigate(shell.FilesPage)
		app.RevealDetails()
	} else {
		app.RevealConversationPanel()
		app.Panel.Show(panel.Tab{Kind: panel.Files})
	}

	wakeup := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wakeup <- struct{}{}:
		default:
		}
	})
	tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")
	pending := map[string]workspaceDirectoryRead{}
	wait := func(stage string, ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(2 * time.Second)
		defer deadline.Stop()
		for {
			tester.Frame()
			if ready() {
				return
			}
			select {
			case call := <-transport.reads:
				if call.input.Worktree != worktree || pending[call.input.Path].done != nil {
					t.Fatalf("unexpected directory read: %+v", call.input)
				}
				pending[call.input.Path] = call
			case <-wakeup:
			case <-deadline.C:
				t.Fatalf("%s timed out; pending: %+v; visible text: %q", stage, pending, tester.Texts())
			}
		}
	}
	read := func(path string, after *protocol.DirectoryCursor) workspaceDirectoryRead {
		t.Helper()
		wait(fmt.Sprintf("directory %q after %+v", path, after), func() bool { return pending[path].done != nil })
		call := pending[path]
		if (call.input.After == nil) != (after == nil) || after != nil && *call.input.After != *after {
			t.Fatalf("expected directory %q after %+v, pending: %+v", path, after, pending)
		}
		delete(pending, path)
		return call
	}
	reply := func(call workspaceDirectoryRead, entries []protocol.FileEntry, next *protocol.DirectoryCursor) {
		call.done <- link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.Directory{Path: call.input.Path, Entries: entries, Next: next}}}
	}
	text := func(key string) string { return i18n.In("en", key) }
	hasProgress := func() bool {
		header, found := tester.Find(text("file_tree"))
		if !found {
			t.Fatal("the Files tree is not shown")
		}
		for _, element := range tester.AccessAll() {
			bounds := element.Bounds
			if element.Role == ui.RoleProgress && bounds.W > 0 && bounds.H > 0 && bounds.X >= header.X && bounds.X < header.X+header.W && bounds.Y >= header.Y+header.H {
				return true
			}
		}
		return false
	}
	row := func(name string) ui.AccessInfo {
		t.Helper()
		info, found := tester.AccessByRole(ui.RoleTreeItem, name)
		if !found {
			t.Fatalf("no tree row %q: %q", name, tester.Texts())
		}
		return info
	}
	invalidate := func() {
		transport.files.updates <- protocol.Update{Kind: "files_changed", Data: protocol.UpdateFilesChangedPayload{Node: node, Worktree: worktree}}
	}

	first := read("", nil)
	workspace := r.selectedWorkspace()
	if workspace == nil {
		t.Fatal("the fixture did not select a workspace")
	}
	awaitListing := func(path string) {
		t.Helper()
		wait(fmt.Sprintf("completion of directory %q", path), func() bool { return !workspace.loading["directory/"+path] })
	}
	if !hasProgress() || tester.HasText(text("files_directory_empty")) {
		t.Fatal("the first listing did not show its loading state")
	}
	reply(first, []protocol.FileEntry{}, nil)
	awaitListing("")
	invalidate()
	root := read("", nil)
	if !tester.HasText(text("files_directory_empty")) || hasProgress() {
		t.Fatal("refresh replaced the already loaded empty directory")
	}

	rootEntries := []protocol.FileEntry{{Name: "plans", Kind: protocol.EntryKindDirectory}}
	reply(root, rootEntries, nil)
	awaitListing("")
	plans := row("plans")
	tester.ClickAt(plans.Bounds.X+plans.Bounds.W/2, plans.Bounds.Y+plans.Bounds.H/2)
	child := read("plans", nil)
	if !hasProgress() {
		t.Fatal("the newly expanded directory did not show its first loading row")
	}

	entries := make([]protocol.FileEntry, 32)
	for index := range entries {
		entries[index] = protocol.FileEntry{Name: fmt.Sprintf("plan-%02d.md", index), Kind: protocol.EntryKindFile}
	}
	page1 := &protocol.DirectoryCursor{Revision: "old", Name: entries[3].Name}
	reply(child, entries[:4], page1)
	awaitListing("plans")
	if err := tester.Click(text("files_load_more")); err != nil {
		t.Fatal(err)
	}
	page2 := &protocol.DirectoryCursor{Revision: "old", Name: entries[len(entries)-1].Name}
	reply(read("plans", page1), entries[4:], page2)
	awaitListing("plans")
	if !row("plans").Expanded {
		t.Fatal("the fixture folder did not open")
	}
	const selected = "plan-31.md"
	if bounds, found := tester.Find(selected); found && bounds.W > 0 && bounds.H > 0 {
		t.Fatal("the fixture must require scrolling to the last row")
	}
	tester.Wheel(plans.Bounds.X+plans.Bounds.W/2, plans.Bounds.Y+plans.Bounds.H/2, 0, 2000)
	tester.Advance(400 * time.Millisecond)
	tester.Frame()
	anchor := row(selected).Bounds
	tester.ClickAtWith(ui.Cmd, anchor.X+anchor.W/2, anchor.Y+anchor.H/2)
	tester.Frame()
	before := row(selected)
	if !before.Selected || before.Bounds.H == 0 {
		t.Fatalf("the fixture did not select and scroll to the row: %+v", before)
	}
	assertStable := func(refreshing bool) {
		t.Helper()
		current := row(selected)
		if hasProgress() {
			t.Fatal("refresh inserted a loading row into the populated tree")
		}
		if refreshing {
			if next, found := tester.AccessByRole(ui.RoleButton, text("files_load_more")); !found || !next.Disabled {
				t.Fatal("refresh replaced the directory's existing pagination row")
			}
		}
		if !current.Selected || current.Bounds != before.Bounds {
			t.Fatalf("refresh changed the visible selection or scroll position: before %+v, after %+v", before, current)
		}
	}
	invalidate()
	root, child = read("", nil), read("plans", nil)
	assertStable(true)
	reply(root, rootEntries, nil)
	fresh := &protocol.DirectoryCursor{Revision: "new", Name: entries[3].Name}
	reply(child, entries[:4], fresh)
	child = read("plans", fresh)
	assertStable(true)
	const added = "session-created.md"
	reply(child, append(entries[4:], protocol.FileEntry{Name: added, Kind: protocol.EntryKindFile}), nil)
	awaitListing("plans")
	assertStable(false)
	if workspaceFileNode(workspace.data.Files, "plans/"+added) == nil || row(added).Bounds.H == 0 {
		t.Fatal("the file-change observation did not publish the new listing")
	}

	invalidate()
	root, child = read("", nil), read("plans", nil)
	root.done <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "directory access revoked"}}
	awaitListing("")
	reply(child, entries, nil)
	awaitListing("plans")
	if tester.HasText("plans") || tester.HasText(text("files_directory_empty")) || !tester.HasText(text("directory_denied")) || !tester.HasText(text("refresh")) || len(workspace.directoryEntries) != 0 {
		t.Fatalf("a forbidden listing showed old rows or an empty result: %q", tester.Texts())
	}
}
