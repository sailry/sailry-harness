package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type resourcePageState struct {
	target shell.ResourceTarget
	shown  *workspaceProject
}

func (r *Runtime) resourceWorkspaceFrame(chat *Runtime) {
	target := r.App.ResourceTarget()
	if r.App.DemoOn() {
		target = shell.ResourceTarget{}
	}
	shown := r.resolveResourceWorkspace(target)
	chatWorkspace := chat.selectedWorkspace()
	if previous := r.resourcePage.shown; previous != nil && previous != shown && previous != chatWorkspace {
		previous.releaseView()
	}
	r.resourcePage = resourcePageState{target: target, shown: shown}
	if shown != nil && shown != chatWorkspace {
		shown.pruneDocumentImages()
		shown.resourceFrame()
	}
}

func (r *Runtime) resolveResourceWorkspace(target shell.ResourceTarget) *workspaceProject {
	if target.Project == "" {
		return nil
	}
	project := r.App.ResourceProject()
	if project == nil || project.Offline {
		return nil
	}
	owner := r.projectOverviewRuntime(project)
	if owner == nil || !owner.controller.Connected() {
		return nil
	}
	var tree protocol.Worktree
	var found bool
	if target.Worktree == "" {
		tree, found = owner.projectWorktree(project, "")
	} else if id, err := protocol.ParseWorktreeID(target.Worktree); err == nil {
		tree, found = owner.workspaceWorktree(id)
	}
	if !found || tree.Project == nil || tree.Project.String() != target.Project {
		return nil
	}
	workspace := owner.workspaceFor(tree)
	workspace.owner = tree
	return workspace
}

// A header switch can happen after the frame resolved its workspace.
func (r *Runtime) resourceWorkspace(target shell.ResourceTarget) *panel.Project {
	if shown := r.resourcePage.shown; r.resourcePage.target == target && shown != nil && shown.runtime.controller.Connected() {
		return shown.panel
	}
	return nil
}

func (r *Runtime) showingResourcePage() bool {
	return r.App.Page == shell.FilesPage || r.App.Page == shell.GitPage
}

func (r *Runtime) shownResourceWorkspace() *workspaceProject {
	if !r.showingResourcePage() {
		return nil
	}
	if target := r.App.ResourceTarget(); target.Project != "" {
		if r.resourceWorkspace(target) != nil {
			return r.resourcePage.shown
		}
		return nil
	}
	return r.selectedRuntime().selectedWorkspace()
}

func (p *workspaceProject) openResourceWorktree(c *ui.Context, path string) {
	owner := p.runtime
	root := owner.localRuntime()
	if root.shownResourceWorkspace() != p || p.owner.Project == nil {
		kit.Toast(c, kit.Conflict, i18n.T(c, "worktree_context_changed"))
		return
	}
	project := *p.owner.Project
	selectTree := func(tree protocol.Worktree) {
		root.App.SetResourceTarget(shell.ResourceTarget{Node: owner.controller.Target().String(), Project: project.String(), Worktree: tree.ID.String()})
	}
	if tree, ok := p.registeredWorktree(path); ok {
		selectTree(tree)
		return
	}
	page, target := root.App.Page, root.App.ResourceTarget()
	p.mutateNamed(worktreeOpen, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project, Path: path}}, func(_ *ui.Context, output protocol.Output) {
		tree, ok := detail[protocol.Worktree](output.Data)
		if !ok || tree.Project == nil || *tree.Project != project {
			return
		}
		owner.workspace.admitted[tree.ID] = tree
		p.locationsLoaded = false
		if root.App.Page == page && root.App.ResourceTarget() == target && root.shownResourceWorkspace() == p {
			selectTree(tree)
		}
	})
}

// showItem selects a file or diff in its workspace. Only the current
// conversation workspace may expose it as a conversation panel tab.
func (p *workspaceProject) showItem(kind panel.Kind, id string) {
	root := p.runtime.localRuntime()
	app := root.App
	if kind == panel.Files {
		p.panel.Files.Show(id)
	} else {
		p.panel.Git.Show(id)
	}
	if root.showingResourcePage() || root.selectedRuntime().selectedWorkspace() != p {
		return
	}
	app.RevealConversationPanel()
	app.Panel.Show(panel.Tab{Kind: kind, ID: id})
}
