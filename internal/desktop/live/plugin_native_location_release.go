package live

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (location *pluginNativeLocation) releasePresentation(tree protocol.WorktreeID) {
	location.state.runtime.releaseWorkspacePresentation(tree)
}

func (owner *Runtime) releaseWorkspacePresentation(tree protocol.WorktreeID) {
	root := owner.localRuntime()
	if workspace := owner.workspace; workspace != nil {
		if project := workspace.projects[tree]; project != nil {
			retained := project.hasFileDrafts() || len(project.pendingWorkspaceOperations()) > 0 ||
				project.git.HasPending != nil && project.git.HasPending()
			for key := range owner.busy {
				if strings.HasPrefix(key, "workspace-action/"+tree.String()+"\x00") || strings.HasPrefix(key, "workspace-workflow/"+tree.String()+"/") {
					retained = true
					break
				}
			}
			project.closeFileDirectory()
			project.releaseResourceWatch()
			if !retained {
				project.closeDocumentImages()
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
	root.wake()
}
