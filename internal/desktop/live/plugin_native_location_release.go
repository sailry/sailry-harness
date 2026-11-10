package live

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (location *pluginNativeLocation) releasePresentation(tree protocol.WorktreeID) {
	owner := location.state.runtime
	root := owner.localRuntime()
	if workspace := owner.workspace; workspace != nil {
		if project := workspace.projects[tree]; project != nil {
			retained := location.worktreeDrafts(tree) || len(project.pendingWorkspaceOperations()) > 0
			for key := range owner.busy {
				if strings.HasPrefix(key, "workspace-action/"+tree.String()+"\x00") {
					retained = true
					break
				}
			}
			project.panel.ResetFileSearch()
			project.stopGitRefreshTimer()
			if project.cancel != nil {
				project.cancel()
				project.cancel = nil
			}
			if project.release != nil {
				project.release()
				project.release = nil
			}
			if !retained {
				delete(workspace.projects, tree)
			}
		}
		delete(workspace.admitted, tree)
		if workspace.selected == tree {
			workspace.selected, workspace.session = protocol.WorktreeID{}, protocol.SessionID{}
			if root.selectedRuntime() == owner {
				root.App.Panel.Actions = nil
				root.App.Panel.Data = &viewmodel.Workspace{}
			}
		}
	}
	if resources := root.pluginResources; resources != nil {
		node := owner.controller.Target()
		for key, pane := range resources.panes {
			if key.node == node && key.worktree == tree {
				root.detachPluginResource(pane)
			}
		}
		for key := range resources.titles {
			if key.resource.node == node && key.resource.worktree == tree {
				delete(resources.titles, key)
			}
		}
		for key := range resources.pending {
			if key.node == node && key.worktree == tree {
				delete(resources.pending, key)
			}
		}
	}
	root.wake()
}
