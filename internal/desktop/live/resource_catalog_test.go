package live

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResourceDirectoryProjectsBeforeRemoteClient(t *testing.T) {
	owner, project, worktree := protocol.NodeID{2}, protocol.ProjectID{3}, protocol.WorktreeID{4}
	synced := uint64(12345)
	source := protocol.ResourceSource{Node: owner, Revision: 9, LastSyncedAtMs: &synced,
		Projects:  []protocol.Project{{ID: project, Name: "Cached project", Path: "/remote/project"}},
		Worktrees: []protocol.Worktree{{ID: worktree, Project: &project, Path: "/remote/project", Main: true}},
		Fault:     &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "offline"}}
	data := resourceSidebar(source, protocol.Snapshot{Node: owner}, false, preferences.Default())
	if len(data.Projects) != 1 {
		t.Fatalf("cold directory = %#v", data.Projects)
	}
	shown := data.Projects[0]
	if shown.Host != owner.String() || shown.ID != project.String() || shown.Path != "/remote/project" || !shown.Offline ||
		!shown.LastSyncedAt.Equal(time.UnixMilli(int64(synced))) || len(shown.Worktrees) != 1 {
		t.Fatalf("cold directory lost identity or availability: %#v", shown)
	}
	// A complete observation newer than the last durable catalog must not
	// disappear merely because its connection was interrupted afterwards.
	snapshot := protocol.Snapshot{Node: owner, Cursor: 10, Projects: []protocol.Project{{ID: project, Name: "Newer project", Path: "/remote/project"}}}
	if shown := resourceSidebar(source, snapshot, false, preferences.Default()).Projects[0]; shown.Name != "Newer project" || !shown.Offline {
		t.Fatalf("offline catalog replaced a newer complete snapshot: %#v", shown)
	}
}

func TestResourceDirectoryReconnectKeepsDraftAndResourceIdentity(t *testing.T) {
	local, remote, other := protocol.NodeID{1}, protocol.NodeID{2}, protocol.NodeID{3}
	project := protocol.ProjectID{4}
	controller := client.New(&conversationTestTransport{snapshot: protocol.Snapshot{Node: local}})
	t.Cleanup(func() { _ = controller.Close() })
	prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
	r := &Runtime{ctx: t.Context(), controller: controller, Preferences: prefs, App: shell.NewLive(Sidebar(protocol.Snapshot{Node: local}, prefs.Data))}
	r.resourceCatalog = &protocol.ResourceCatalog{Entry: local, Sources: []protocol.ResourceSource{
		{Node: remote, Projects: []protocol.Project{{ID: project, Name: "Remote", Path: "/remote"}}, Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}},
		{Node: other, Projects: []protocol.Project{{ID: project, Name: "Other", Path: "/other"}}},
	}}
	projectData := func() {
		data := Sidebar(protocol.Snapshot{Node: local}, prefs.Data)
		r.appendResourceDirectory(data)
		r.applyConversationData(data)
	}
	projectData()
	if len(r.App.Data.Projects) != 2 || r.App.Data.Projects[0].Host == r.App.Data.Projects[1].Host {
		t.Fatal("equal IDs from different sources were merged")
	}
	r.newDraft(r.App.Data.Projects[0])
	selected := r.App.Sidebar.Selected
	draft := r.App.Composer(selected)
	draft.Draft = "Keep my local input"
	r.resourceCatalog.Sources[0].Fault = nil
	r.resourceCatalog.Sources[0].Projects[0].Name = "Updated remote"
	projectData()
	if len(r.App.Data.Projects) != 2 || r.App.Data.Projects[0].Name != "Updated remote" || r.App.Sidebar.Selected != selected ||
		r.App.Composer(selected) != draft || draft.Draft != "Keep my local input" || r.App.Data.ProjectOf(selected).Host != remote.String() {
		t.Fatal("directory update duplicated resources, moved selection or replaced the draft")
	}
	// Revocation removes the restricted resource even if a stale direct
	// Client could still have supplied an in-memory snapshot.
	r.resourceCatalog.Sources[0].Fault = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied}
	projectData()
	if len(r.App.Data.Projects) != 1 || r.resourceVisible(remote.String()) {
		t.Fatal("revoked directory remains visible")
	}
}

func TestOfflineResourceCommandDoesNotEnterTransport(t *testing.T) {
	transport := &conversationTestTransport{snapshot: protocol.Snapshot{Node: protocol.NodeID{2}}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	root := &Runtime{}
	remote := &Runtime{parent: root, controller: controller}
	called := false
	id := remote.executeScoped("offline-save", protocol.Command{Kind: "write_file"}, func(_ *ui.Context, _ protocol.Output, err error) {
		called = true
		if faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("offline command = %v", err)
		}
	})
	for _, apply := range root.queue {
		apply(nil)
	}
	if !called || id != (protocol.RequestID{}) || len(transport.requests) != 0 || len(remote.uncertain) != 0 {
		t.Fatal("offline command was admitted, queued or reported as successful")
	}
}

func TestOfflineRunShowsLastKnownStatus(t *testing.T) {
	cv := &viewmodel.Conversation{Offline: true, Status: viewmodel.Running, Animating: true,
		Turns: []conversation.Turn{{Reply: conversation.Reply{Status: conversation.Running}}}}
	markOfflineConversation(cv)
	if cv.Status == viewmodel.Running || cv.Animating || cv.Session.Connection != conversation.Offline || cv.Turns[0].Reply.Status != conversation.Unsynced {
		t.Fatal("cached run is presented as a currently confirmed running task")
	}
}

func TestBindingReleasedWhenControllerDirectionDisappears(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	local, remote := protocol.NodeID{1}, protocol.NodeID{2}
	controller := client.New(&conversationTestTransport{snapshot: protocol.Snapshot{Node: local}})
	t.Cleanup(func() { _ = controller.Close() })
	prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
	root := &Runtime{ctx: ctx, controller: controller, Preferences: prefs, App: shell.NewLive(Sidebar(protocol.Snapshot{Node: local}, prefs.Data)),
		network: &Network{peers: map[protocol.NodeID]*networkPeer{remote: {address: link.EndpointAddress{ID: remote}}}}}
	remoteClient := client.New(&conversationTestTransport{snapshot: protocol.Snapshot{Node: remote}})
	t.Cleanup(func() { _ = remoteClient.Close() })
	boundContext, boundCancel := context.WithCancel(ctx)
	bound := &Runtime{parent: root, ctx: boundContext, cancel: boundCancel, controller: remoteClient, App: root.App, Preferences: prefs}
	root.bindings = map[string]*Runtime{remote.String(): bound}
	root.syncNetwork()
	if root.bindings[remote.String()] != nil || !bound.closed || boundContext.Err() == nil {
		t.Fatal("a peer without an outbound Client retained its old Runtime binding")
	}
}

func TestConfirmedDirectoryRemovalInvalidatesOpenResourceViews(t *testing.T) {
	local, remote := protocol.NodeID{1}, protocol.NodeID{2}
	projectID, worktreeID := protocol.ProjectID{3}, protocol.WorktreeID{4}
	controller := client.New(&conversationTestTransport{snapshot: protocol.Snapshot{Node: remote}})
	t.Cleanup(func() { _ = controller.Close() })
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	prefs := preferences.Default()
	root := &Runtime{App: shell.NewLive(Sidebar(protocol.Snapshot{Node: local}, prefs))}
	owner := &Runtime{parent: root, ctx: ctx, controller: controller, App: root.App}
	owner.initWorkspace()
	worktree := protocol.Worktree{ID: worktreeID, Project: &projectID, Path: "/remote/private"}
	project := owner.workspaceFor(worktree)
	owner.workspace.selected = worktreeID
	readCtx, readCancel := context.WithCancel(ctx)
	project.cancel = readCancel
	released := 0
	project.release = func() { released++ }
	project.panel.SetFile("private.txt", "private contents\n")
	project.panel.FileChanged("private.txt", "local draft")
	preview, previewCtx := owner.beginFilePreview(conversation.File{Path: "private.txt", Text: "private contents"}, fileview.Callbacks{})
	preview.worktree = worktreeID
	root.bindings = map[string]*Runtime{remote.String(): owner}
	synced := uint64(1234)
	root.resourceCatalog = &protocol.ResourceCatalog{Sources: []protocol.ResourceSource{{Node: remote, Revision: 1,
		LastSyncedAtMs: &synced, Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}}}
	root.pruneResourceViews()
	if owner.workspace.projects[worktreeID] != project || readCtx.Err() != nil || root.filePreview != preview || previewCtx.Err() != nil {
		t.Fatal("an unavailable or partial source discarded its open projection")
	}
	if draft, dirty := project.panel.FileDraft("private.txt"); !dirty || draft != "local draft\n" {
		t.Fatal("offline transition discarded the local file draft")
	}
	root.resourceCatalog.Sources[0].Fault = nil
	root.pruneResourceViews()
	root.pruneResourceViews()
	if owner.workspace.projects[worktreeID] != nil || owner.workspace.selected != (protocol.WorktreeID{}) ||
		readCtx.Err() == nil || previewCtx.Err() == nil || released != 1 || root.filePreview != nil || preview.file.Text != "" || root.App.Panel.Actions != nil {
		t.Fatal("confirmed directory removal retained an open restricted projection")
	}
}

func TestFilePreviewInvalidationRetainsAnotherOwner(t *testing.T) {
	root := &Runtime{ctx: t.Context()}
	first := &Runtime{parent: root, ctx: t.Context()}
	second := &Runtime{parent: root, ctx: t.Context()}
	preview, ctx := first.beginFilePreview(conversation.File{Path: "first.txt", Text: "first"}, fileview.Callbacks{})
	second.invalidateFilePreview(nil)
	if root.filePreview != preview || ctx.Err() != nil {
		t.Fatal("revoking one owner cleared another owner's preview")
	}
	first.invalidateFilePreview(nil)
	if root.filePreview != nil || ctx.Err() == nil || preview.file.Text != "" {
		t.Fatal("revoked owner's preview was retained")
	}
}

func TestProjectOverviewReentryRefreshesUnobservedRepository(t *testing.T) {
	node, projectID, worktreeID := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: projectID, Name: "Project", Path: "/fixture/project"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktreeID, Project: &projectID, Path: "/fixture/project", Main: true}}
	source := &conversationTestTransport{snapshot: snapshot}
	branch, inspections := "main", 0
	source.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "inspect_git" {
			return link.Response{}, false
		}
		inspections++
		current := branch
		return link.Response{Output: protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory, Branch: &current}}}, true
	}
	controller := client.New(source)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	prefs := preferences.Default()
	r := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLiveWithAssembly(Sidebar(snapshot, prefs), testassembly.Full()),
		busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	r.initWorkspace()
	settle := func() {
		for range 3 {
			r.workers.Wait()
			r.mu.Lock()
			queue := r.queue
			r.queue = nil
			r.mu.Unlock()
			for _, apply := range queue {
				apply(nil)
			}
		}
	}
	r.openProjectOverview(r.App.Data.Projects[0])
	settle()
	workspace := r.workspace.projects[worktreeID]
	if workspace == nil || workspace.release != nil || workspaceString(workspace.status.Branch) != "main" || inspections != 1 {
		t.Fatal("overview did not capture the initial unobserved repository")
	}
	r.App.Navigate(shell.ConversationPage)
	source.mu.Lock()
	branch = "changed-externally"
	source.mu.Unlock()
	r.openProjectOverview(r.App.Data.Projects[0])
	settle()
	if r.workspace.projects[worktreeID] != workspace || workspaceString(workspace.status.Branch) != "changed-externally" || inspections != 2 {
		t.Fatal("reentering the overview reused stale Git state")
	}
}
