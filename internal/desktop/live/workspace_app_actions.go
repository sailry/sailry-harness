package live

import (
	"encoding/json"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const workspaceOpenFileAction shell.AppActionID = "workspace.open-file"

type workspaceOpenFileInput struct {
	Path string `json:"path"`
	Line int    `json:"line,omitempty"`
}

func (r *Runtime) bindWorkspaceAppActions() error {
	_, err := r.App.RegisterAppAction(shell.AppAction{
		ID: workspaceOpenFileAction, PublicNavigation: true, PluginPermission: protocol.PluginActionReadFiles,
		Run: r.openWorkspaceFileAction,
	})
	return err
}

func (r *Runtime) openWorkspaceFileAction(_ *ui.Context, app *shell.App, source shell.AppActionContext, body json.RawMessage) error {
	var input workspaceOpenFileInput
	if protocol.DecodeObject(body, &input, []string{"path"}, true) != nil || !validWorkspaceRelative(input.Path) ||
		strings.ContainsFunc(input.Path, unicode.IsControl) || input.Line < 0 {
		return fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid workspace file action input")
	}
	unavailable := func() error {
		return fileActionFault(protocol.ErrorCodeUnavailable, "captured file workspace is unavailable")
	}
	if source.Node == (protocol.NodeID{}) || source.Worktree == (protocol.WorktreeID{}) {
		return unavailable()
	}
	node, sessionID := source.Node.String(), source.Session.String()
	owner := r.runtimeFor(node)
	if owner == nil || owner.ctx.Err() != nil || owner.workspace == nil || !owner.controller.Connected() || !r.resourceVisible(node) {
		return unavailable()
	}
	tree, exists := owner.workspaceWorktree(source.Worktree)
	if !exists || tree.Path == "" {
		return unavailable()
	}
	hasSession := source.Session != (protocol.SessionID{})
	if hasSession {
		session, found := owner.sessionWithID(source.Session)
		if !found || session.Worktree != tree.ID {
			return fileActionFault(protocol.ErrorCodeRevisionConflict, "workspace association changed")
		}
	}
	var project *workspaceProject
	resourcePage := r.showingResourcePage()
	switch {
	case resourcePage:
		if app.ResourceKind() == panel.Files {
			project = r.shownResourceWorkspace()
		}
	case hasSession:
		if app.ActiveTerminal() == "" && app.Data.FindOn(node, sessionID) != nil {
			project = owner.workspaceFor(tree)
		}
	case r.selectedRuntime() == owner:
		project = owner.selectedWorkspace()
	}
	if project == nil || project.runtime != owner || project.owner.ID != tree.ID || project.owner.Path != tree.Path {
		return unavailable()
	}
	if project.trashBlocksRead("file/"+input.Path) || project.entryIsMissing(input.Path) {
		return fileActionFault(protocol.ErrorCodeNotFound, "workspace file is unavailable")
	}
	project.openFile(input.Path, input.Line, false, func(relative string, line int) {
		if !source.Current() || r.runtimeFor(node) != owner || !owner.controller.Connected() || !r.resourceVisible(node) {
			return
		}
		if now, ok := owner.workspaceWorktree(tree.ID); !ok || now.Path != tree.Path {
			return
		}
		if resourcePage {
			if app.ResourceKind() == panel.Files && r.shownResourceWorkspace() == project {
				project.showFileEditor(relative, line)
			}
			return
		}
		if hasSession {
			current, ok := owner.sessionWithID(source.Session)
			cv := app.Data.FindOn(node, sessionID)
			if !ok || current.Worktree != tree.ID || cv == nil || app.ActiveTerminal() != "" {
				return
			}
			if app.SelectedConversation() != cv || owner.selectedWorkspace() != project {
				app.Sidebar.SelectOn(node, sessionID)
				app.OpenConversation(sessionID)
				owner.workspaceFrame(nil)
			}
		}
		if r.selectedRuntime() == owner && owner.selectedWorkspace() == project && !r.showingResourcePage() {
			project.showFileEditor(relative, line)
		}
	})
	return nil
}

func (r *Runtime) openWorkspaceFile(session protocol.SessionID, worktree protocol.WorktreeID, relative string) {
	app := r.App
	overview := r.overviews[session]
	if overview == nil || !overview.open {
		return
	}
	generation := overview.generation
	selected, node := app.Sidebar.Selected, app.Sidebar.SelectedNode()
	source := app.BindAppActions(shell.AppActionScope{Node: r.controller.Target(), Worktree: worktree, Session: session}, func() bool {
		return r.workflowWindowCurrent() && app.Page == shell.ConversationPage && app.ActiveTerminal() == "" &&
			app.Sidebar.Selected == selected && app.Sidebar.SelectedNode() == node && r.overviews[session] == overview &&
			overview.open && overview.generation == generation
	})
	input, _ := json.Marshal(workspaceOpenFileInput{Path: relative})
	if err := source.Invoke(nil, workspaceOpenFileAction, input); err != nil {
		r.enqueue(func(c *ui.Context) {
			r.reportKeys(c, err, "files_open_failed", "workspace_changed", "workspace_action_unknown")
		})
	}
}
