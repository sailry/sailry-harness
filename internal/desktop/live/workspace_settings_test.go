package live

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceHookSettings(t *testing.T) {
	for _, admitted := range []bool{false, true} {
		name := "snapshot worktree"
		if admitted {
			name = "newly admitted worktree"
		}
		t.Run(name, func(t *testing.T) {
			node, project, worktree := protocol.NodeID{1}, protocol.NewProjectID(), protocol.NewWorktreeID()
			tree := protocol.Worktree{ID: worktree, Project: &project, Path: "/fixture/hooks", Main: true}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			if !admitted {
				snapshot.Worktrees = []protocol.Worktree{tree}
			}
			controller := client.New(&conversationTestTransport{snapshot: snapshot})
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			const draftID = "draft/hooks"
			data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}, Projects: []*viewmodel.Project{{ID: project.String(), Conversations: []*viewmodel.Conversation{{ID: draftID}}}}}
			r := &Runtime{controller: controller, App: shell.NewLive(data), workspace: &workspaceState{admitted: map[protocol.WorktreeID]protocol.Worktree{}}}
			if admitted {
				r.workspace.admitted[worktree] = tree
			}
			r.App.Sidebar.Selected = draftID
			r.App.Composer(draftID).Worktree = tree.Path
			r.workspaceSettings = workspaceSettingsState{worktree: worktree, sandboxed: true}
			r.App.Settings.Hooks = settings.Stored[settings.HookSettings]{Revision: 7}
			r.workspaceSettingsFrame()
			if r.workspaceSettings.worktree != worktree || r.App.Settings.Hooks.Revision != 7 || !r.hooksSandboxed() {
				t.Fatal("project draft lost its captured hook owner or settings")
			}
			label, ok := r.hookContext()
			if !ok || !strings.Contains(label, node.String()) || !strings.Contains(label, tree.Path) || !strings.Contains(label, worktree.String()) {
				t.Fatalf("hook consent scope = %q, available=%v", label, ok)
			}
			delete(r.workspace.admitted, worktree)
			r.App.Data.Projects = nil
			r.App.Data.Unassigned = []*viewmodel.Conversation{{ID: "draft/unassigned"}}
			r.App.Sidebar.Selected = "draft/unassigned"
			r.workspaceSettingsFrame()
			if r.workspaceSettings.worktree != (protocol.WorktreeID{}) || r.hooksSandboxed() || r.App.Settings.Hooks.Revision != 0 {
				t.Fatal("unassigned draft retained project hook authority")
			}
			if _, ok := r.hookContext(); ok {
				t.Fatal("unassigned draft exposed project hook consent")
			}
		})
	}
}
