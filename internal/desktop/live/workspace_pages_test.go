package live

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	desktop "github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type resourcePageTransport struct{ conversationTestTransport }

func (transport *resourcePageTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "files" || topic.Kind == "repository" {
		return &conversationTestSubscription{closed: make(chan struct{})}, nil
	}
	return transport.conversationTestTransport.Subscribe(ctx, topic)
}

func TestResourcePagesKeepTheirOwnWorktree(t *testing.T) {
	localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
	projectID := protocol.ProjectID{3}
	mainID, linkedID := protocol.WorktreeID{4}, protocol.WorktreeID{5}
	main, linked := "/remote/project", "/remote/linked"
	localProject := &viewmodel.Project{ID: projectID.String(), Host: localNode.String(), Name: "Local", Path: "/local/project"}
	remoteProject := &viewmodel.Project{ID: projectID.String(), Host: remoteNode.String(), Name: "Runner", Path: main}
	chat := &viewmodel.Conversation{ID: "draft/local", Node: localNode.String(), Title: "Local draft"}
	localProject.Conversations = []*viewmodel.Conversation{chat}
	assembly := shell.Assemble(desktop.RegisterFilesFixture)
	changes := panel.ResourceOf(panel.Changes)
	changes.Render = panel.RenderProjectResource
	changes.BindWorkspace = func(_ *panel.Project, host panel.GitWorkspaceHost) panel.GitWorkspaceBinding {
		inspected := false
		return panel.GitWorkspaceBinding{Frame: func(bool) {
			if inspected {
				return
			}
			inspected = true
			command := protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: host.Worktree}}
			host.Read(host.Context, command, func(_ *ui.Context, out protocol.Output, err error) {
				status, ok := detail[protocol.GitStatus](out.Data)
				host.PublishStatus(status, err == nil && ok)
			})
		}}
	}
	assembly.RegisterPanelResource(changes)
	assembly.WorkspacePages = append(assembly.WorkspacePages, shell.NativePage{Page: shell.GitPage, Resource: panel.Changes,
		Feature: shell.Feature{ID: "sample:repository", Label: "git", Icon: changes.Icon}, Render: func(*ui.Context, *shell.App) {}})
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Projects: []*viewmodel.Project{localProject, remoteProject}, Workspace: &viewmodel.Workspace{}}, assembly)
	app.Sidebar.Selected = chat.ID
	composer := app.Composer(chat.ID)
	composer.Draft = "Keep this input"
	composer.Worktree = localProject.Path

	fixture := func(parent *Runtime, node protocol.NodeID, path string, extra ...protocol.Worktree) (*Runtime, *resourcePageTransport) {
		t.Helper()
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		snapshot.Projects = []protocol.Project{{ID: projectID, Path: path}}
		snapshot.Worktrees = append([]protocol.Worktree{{ID: mainID, Project: &projectID, Path: path, Main: true}}, extra...)
		transport := &resourcePageTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			var output protocol.Output
			switch request.Command.Kind {
			case "list_directory":
				input, _ := detail[protocol.CommandListDirectoryPayload](request.Command.Data)
				entry := "remote-main.txt"
				if node == localNode {
					entry = "local-main.txt"
				} else if input.Worktree == linkedID {
					entry = "remote-linked.txt"
				}
				output = protocol.Output{Kind: "directory", Data: protocol.Directory{Entries: []protocol.FileEntry{{Name: entry, Kind: protocol.EntryKindFile}}}}
			case "inspect_git":
				output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}
			}
			return link.Response{Output: output}, output.Kind != ""
		}
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		owner := &Runtime{parent: parent, ctx: t.Context(), controller: controller, App: app, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
		owner.initWorkspace()
		t.Cleanup(func() { owner.closeWorkspace(); _ = controller.Close() })
		return owner, transport
	}
	root, localCalls := fixture(nil, localNode, localProject.Path)
	remote, remoteCalls := fixture(root, remoteNode, main, protocol.Worktree{ID: linkedID, Project: &projectID, Path: linked})
	root.bindings = map[string]*Runtime{remoteNode.String(): remote}
	local := root.workspaceFor(protocol.Worktree{ID: mainID, Project: &projectID, Path: localProject.Path, Main: true})
	root.workspace.selected = mainID

	target := shell.ResourceTarget{Node: remoteNode.String(), Project: projectID.String()}
	app.Navigate(shell.FilesPage)
	app.SetResourceTarget(target)
	load := func(worktree protocol.WorktreeID, entry string) *workspaceProject {
		t.Helper()
		root.resourceWorkspaceFrame(root)
		remote.workers.Wait()
		pluginViewRunQueue(root)
		workspace := root.shownResourceWorkspace()
		if workspace == nil || workspace.runtime != remote || workspace.owner.ID != worktree {
			t.Fatalf("wrong resource workspace for %s: %+v", worktree, workspace)
		}
		return workspace
	}
	workspace := load(mainID, "remote-main.txt")
	app.Navigate(shell.GitPage)
	root.resourceWorkspaceFrame(root)
	remote.workers.Wait()
	pluginViewRunQueue(root)
	if root.shownResourceWorkspace() != workspace || root.workspaceProject() != local.panel || remote.selectedWorkspace() != nil {
		t.Fatal("Git did not reuse the Files workspace independently of the chat")
	}
	workspace.showItem(panel.Changes, "output")
	if workspace.panel.Git.Change() != "output" || app.PanelOpen {
		t.Fatal("the standalone Git document changed the conversation panel")
	}
	location, ok := workspace.registeredLocation(linked)
	if !ok {
		t.Fatal("the linked location is not registered for the remote project")
	}
	workspace.workflowResourceWorkspace().SelectLocation(nil, location)
	if target = app.ResourceTarget(); target.Worktree != linkedID.String() || root.resourceWorkspace(target) != nil {
		t.Fatal("Git worktree switching exposed the previous workspace")
	}
	linkedWorkspace := load(linkedID, "remote-linked.txt")
	if app.Sidebar.Selected != chat.ID || composer.Draft != "Keep this input" || composer.Worktree != localProject.Path {
		t.Fatal("worktree switching retained its previous subscription or changed the chat")
	}
	type resourceRead struct {
		kind string
		tree protocol.WorktreeID
	}
	reads := make(map[resourceRead]int)
	for _, request := range remoteCalls.requests {
		if request.Target != remoteNode {
			t.Fatalf("resource read targeted another Node: %+v", request)
		}
		switch input := request.Command.Data.(type) {
		case protocol.CommandListDirectoryPayload:
			reads[resourceRead{request.Command.Kind, input.Worktree}]++
		case protocol.CommandInspectGitPayload:
			reads[resourceRead{request.Command.Kind, input.Worktree}]++
		default:
			t.Fatalf("unexpected resource read: %+v", request)
		}
	}
	if len(localCalls.requests) != 0 || len(remoteCalls.requests) != 2 {
		t.Fatalf("expected only Git reads for each remote worktree, got %d local and %d remote", len(localCalls.requests), len(remoteCalls.requests))
	}
	for _, id := range []protocol.WorktreeID{mainID, linkedID} {
		if reads[resourceRead{"list_directory", id}] != 0 || reads[resourceRead{"inspect_git", id}] != 1 {
			t.Fatalf("only Git should read %s once: %v", id, reads)
		}
	}

	remoteProject.Offline = true
	root.resourceWorkspaceFrame(root)
	if root.shownResourceWorkspace() != nil || app.ResourceTarget() != target {
		t.Fatal("offline resource scope retained access, changed target or fell through to the chat")
	}
	app.Navigate(shell.FilesPage)
	target = shell.ResourceTarget{Node: localNode.String(), Project: projectID.String(), Worktree: mainID.String()}
	app.SetResourceTarget(target)
	if root.resourceWorkspace(target) != nil {
		t.Fatal("the target change exposed the previous Node's workspace")
	}
	local.resourceFrame()
	root.resourceWorkspaceFrame(root)
	if root.resourceWorkspace(target) != local.panel || local.data.Files != nil {
		t.Fatal("the new Node showed the previous Node's listing while loading")
	}
	root.workers.Wait()
	pluginViewRunQueue(root)
	if local.data.Files != nil || workspace.data.Files != nil {
		t.Fatal("a resource page fixture started an unmounted browser")
	}
	app.Navigate(shell.ConversationPage)
	linkedWorkspace.showItem(panel.Files, "late.txt")
	if app.PanelOpen || app.Sidebar.Selected != chat.ID || root.workspaceProject() != local.panel {
		t.Fatal("a late workspace action changed the open conversation")
	}
}
