package live

import (
	"path"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// workspaces visits every project this window has opened, on the local Node
// and on each bound one.
func (r *Runtime) workspaces(visit func(*workspaceProject)) {
	root := r.localRuntime()
	owners := []*Runtime{root}
	for _, bound := range root.bindings {
		owners = append(owners, bound)
	}
	for _, owner := range owners {
		if owner.workspace == nil {
			continue
		}
		for _, project := range owner.workspace.projects {
			visit(project)
		}
	}
}

// UnsavedDocuments names the documents of this window whose drafts are not
// saved, as the worktree's folder over the file's path. Closing the window
// or quitting would drop them.
func (r *Runtime) UnsavedDocuments() []string {
	var names []string
	r.workspaces(func(project *workspaceProject) {
		for _, file := range project.panel.Unsaved() {
			names = append(names, path.Join(path.Base(project.owner.Path), file))
		}
	})
	slices.Sort(names)
	return names
}

// SaveDocuments starts saving every unsaved document in this window's next
// frame. Each failed save tells its own toast and keeps its draft.
func (r *Runtime) SaveDocuments() {
	root := r.localRuntime()
	if root.workspace == nil {
		return
	}
	root.workspace.savingAll = true
	root.enqueue(func(c *ui.Context) {
		root.workspace.savingAll = false
		started := true
		root.workspaces(func(project *workspaceProject) {
			if !project.panel.SaveUnsaved(c) {
				started = false
			}
		})
		if !started {
			kit.Toast(c, kit.Failed, i18n.T(c, "workspace_action_failed"))
		}
	})
}

// SavingDocuments reports whether document saves are queued or under way.
func (r *Runtime) SavingDocuments() bool {
	root := r.localRuntime()
	if root.workspace != nil && root.workspace.savingAll {
		return true
	}
	saving := false
	root.workspaces(func(project *workspaceProject) { saving = saving || project.panel.Saving() })
	return saving
}

// LocalWork reports whether the local Node, which stops with the
// application, is running a task or a terminal. Work on a remote Node goes
// on without this controller.
func (r *Runtime) LocalWork() bool { return nodeWorks(r.localRuntime().snapshot()) }

func nodeWorks(snapshot protocol.Snapshot) bool {
	for _, session := range snapshot.Sessions {
		if lane := client.LaneOf(session); lane == client.LaneRunning || lane == client.LaneWaiting {
			return true
		}
	}
	for _, terminal := range snapshot.Terminals {
		if terminal.Status.Kind == "running" {
			return true
		}
	}
	return false
}
