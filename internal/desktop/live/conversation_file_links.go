package live

import (
	"context"
	"os"
	"path"
	"path/filepath"
	"strings"
	"sync/atomic"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) bindConversationLinks() error {
	if err := r.bindReferenceLinks(); err != nil {
		return err
	}
	r.App.Messages.OpenLink = r.openChatLink
	r.nativeWindowResources = append(r.nativeWindowResources, nativeWindowResource{
		beforeFrame: func() {
			if request := r.pendingFileLink; request != nil && !request.current() {
				r.cancelFileLink()
			}
		},
		close: r.cancelFileLink,
	})
	return nil
}

type conversationFileLink struct {
	cancel  context.CancelFunc
	current func() bool
}

func (r *Runtime) cancelFileLink() {
	if request := r.pendingFileLink; request != nil {
		r.pendingFileLink = nil
		request.cancel()
	}
}

func (r *Runtime) openMarkdownFileLink(c *ui.Context, owner *Runtime, target shell.LinkTarget) {
	if owner == nil || target.Worktree == (protocol.WorktreeID{}) {
		r.reportLink(c, unavailableLink(), "files_open_failed")
		return
	}
	tree, found := owner.workspaceWorktree(target.Worktree)
	if !found || tree.Path == "" {
		r.reportLink(c, unavailableLink(), "files_open_failed")
		return
	}
	file, ok := resolveFileLink(target.URL, tree.Path)
	if !ok {
		r.reportLink(c, invalidFileLink(), "files_open_failed")
		return
	}
	target.Path, target.Line = file.path, file.line
	if file.directory {
		target.Route.Kind = shell.LinkDirectory
	}
	if file.workspace && target.Path == "" {
		target.Path, target.Route.Kind = ".", shell.LinkDirectory
	}
	owner.routeFileLink(c, target, tree, file.workspace)
}

func (r *Runtime) openFileReference(c *ui.Context, session string, turn protocol.TurnID, ref protocol.ConversationReference) bool {
	var kind shell.LinkKind
	switch ref.Target.Kind {
	case "file":
		kind = shell.LinkFile
	case "directory":
		kind = shell.LinkDirectory
	default:
		return false
	}
	name, ok := detail[string](ref.Target.Data)
	if !ok || name != "." && !validWorkspaceRelative(name) || name == "." && kind != shell.LinkDirectory || strings.ContainsFunc(name, unicode.IsControl) {
		r.reportLink(c, invalidFileLink(), "files_open_failed")
		return true
	}
	if !r.ownsLinkConversation(session) {
		r.reportLink(c, unavailableLink(), "unavailable")
		return true
	}
	current, found := r.session(session)
	if !found {
		r.reportLink(c, unavailableLink(), "unavailable")
		return true
	}
	worktree := r.fileReferenceWorktree(current, turn)
	if worktree == (protocol.WorktreeID{}) {
		r.reportLink(c, unavailableLink(), "unavailable")
		return true
	}
	tree, found := r.workspaceWorktree(worktree)
	if !found || tree.Path == "" {
		r.reportLink(c, unavailableLink(), "unavailable")
		return true
	}
	r.routeFileLink(c, shell.LinkTarget{
		Route: shell.LinkRoute{Kind: kind}, Node: r.controller.Target(), Session: session,
		Worktree: worktree, Turn: turn, Reference: ref, Path: name, External: conversation.External(c),
	}, tree, true)
	return true
}

func (r *Runtime) fileReferenceWorktree(session protocol.Session, turn protocol.TurnID) protocol.WorktreeID {
	if turn == (protocol.TurnID{}) {
		return session.Worktree
	}
	if state, ok := r.conversationState(session.ID); ok && !state.Removed && state.Snapshot.Node == r.controller.Target() && state.Snapshot.Page.Session == session.ID {
		for _, run := range state.Snapshot.Page.Runs {
			if run.Turn == turn {
				return run.Worktree
			}
		}
	}
	return protocol.WorktreeID{}
}

func (r *Runtime) routeFileLink(c *ui.Context, target shell.LinkTarget, tree protocol.Worktree, scoped bool) {
	root := r.localRuntime()
	root.cancelFileLink()
	local := target.Node != (protocol.NodeID{}) && r.attachments != nil && r.attachments.native.LocalNode == target.Node
	if !scoped && !local {
		r.reportLink(c, unavailableLink(), "files_remote_path")
		return
	}
	currentSession, ok := r.session(target.Session)
	if !ok || scoped && !r.controller.Connected() {
		r.reportLink(c, unavailableLink(), "unavailable")
		return
	}
	app := root.App
	page, selected, resource := app.Page, app.Sidebar.Selected, app.ResourceTarget()
	node, terminal := app.Sidebar.SelectedNode(), app.ActiveTerminal()
	ctx, cancel := context.WithCancel(r.ctx)
	request := &conversationFileLink{cancel: cancel, current: func() bool {
		if ctx.Err() != nil || scoped && !r.controller.Connected() || app.DemoOn() || app.Page != page || app.Sidebar.Selected != selected ||
			app.Sidebar.SelectedNode() != node || app.ResourceTarget() != resource || app.ActiveTerminal() != terminal {
			return false
		}
		if !r.ownsLinkConversation(target.Session) {
			return false
		}
		session, found := r.session(target.Session)
		current, exists := r.workspaceWorktree(tree.ID)
		return found && session.Worktree == currentSession.Worktree && exists && current.Path == tree.Path &&
			(target.Turn == (protocol.TurnID{}) || r.fileReferenceWorktree(session, target.Turn) == tree.ID)
	}}
	root.pendingFileLink = request
	if !r.async(func() {
		var kind shell.LinkKind
		var systemPath string
		var err error
		if scoped {
			kind, err = r.inspectWorkspaceLink(ctx, tree.ID, target.Path)
		} else {
			systemPath, kind, err = localFileLinkPath("", target.Path)
		}
		if err == nil && (target.Route.Kind != "" && target.Route.Kind != kind || kind == shell.LinkDirectory && target.Line != 0) {
			err = invalidFileLink()
		}
		if err == nil {
			target.Route = shell.LinkRoute{Kind: kind}
		}
		r.enqueue(func(c *ui.Context) {
			if root.pendingFileLink != request {
				return
			}
			defer root.cancelFileLink()
			if !request.current() {
				return
			}
			if err != nil {
				r.reportLink(c, err, "files_open_failed")
				return
			}
			r.dispatchFileLink(c, target, tree, scoped, local, systemPath, request.current)
		})
	}) {
		root.cancelFileLink()
		r.reportLink(c, unavailableLink(), "unavailable")
	}
}

func invalidFileLink() error {
	return fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid file or directory link")
}

// inspectWorkspaceLink relies on the captured Node's scoped listing for both
// the target type and authorization. It never treats a failed read as a file.
func (r *Runtime) inspectWorkspaceLink(ctx context.Context, worktree protocol.WorktreeID, name string) (shell.LinkKind, error) {
	directory, base := path.Dir(name), path.Base(name)
	var after *protocol.DirectoryCursor
	for {
		out, err := r.controller.Do(ctx, protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{
			Worktree: worktree, Path: directory, After: after,
		}})
		if err != nil {
			return "", err
		}
		listing, ok := detail[protocol.Directory](out.Data)
		if !ok || out.Kind != "directory" || listing.Path != directory {
			return "", unavailableLink()
		}
		if name == "." {
			return shell.LinkDirectory, nil
		}
		for _, entry := range listing.Entries {
			if entry.Name != base {
				continue
			}
			switch entry.Kind {
			case protocol.EntryKindFile:
				return shell.LinkFile, nil
			case protocol.EntryKindDirectory:
				return shell.LinkDirectory, nil
			default:
				return "", fileActionFault(protocol.ErrorCodePermissionDenied, "link target must be an ordinary file or directory")
			}
		}
		next := listing.Next
		if next == nil {
			return "", fileActionFault(protocol.ErrorCodeNotFound, "link target does not exist in the captured worktree")
		}
		if after != nil && (after.Revision != next.Revision || !after.Directory && next.Directory || after.Directory == next.Directory && after.Name >= next.Name) {
			return "", unavailableLink()
		}
		after = next
	}
}

func (r *Runtime) dispatchFileLink(c *ui.Context, target shell.LinkTarget, tree protocol.Worktree, scoped, local bool, systemPath string, current func() bool) {
	key := "files_open_failed"
	var err error
	switch {
	case !scoped:
		err = r.openSystemLink(systemPath, target.External && target.Route.Kind == shell.LinkFile)
	case local && target.External:
		if target.Route.Kind == shell.LinkFile {
			if r.attachments.native.OpenLocal == nil {
				err = unavailableLink()
			} else {
				r.openExternalAttachment(linkAttachment(target), false)
			}
		} else {
			err = r.revealScopedLink(tree, target)
		}
	default:
		var active, claimed atomic.Bool
		active.Store(true)
		claim := func(kind shell.LinkKind) bool {
			return active.Load() && target.Route.Kind == kind && current() && claimed.CompareAndSwap(false, true)
		}
		target.ConversationWorkspace = r.canOpenLinkInConversation(target)
		actions := shell.LinkActions{
			OpenWorkspaceFile: func(destination shell.LinkFileDestination) error {
				if !claim(shell.LinkFile) {
					return unavailableLink()
				}
				return r.openLinkWorkspaceFile(target, tree, destination)
			},
			ShowWorkspaceDirectory: func(page shell.Page, reveal func(*panel.Project, string)) error {
				if !claim(shell.LinkDirectory) || reveal == nil {
					return unavailableLink()
				}
				project, err := r.showLinkWorkspacePage(page, target, tree)
				if err == nil {
					reveal(project.panel, target.Path)
					r.App.RevealDetails()
				}
				return err
			},
		}
		handled, handlerErr := r.App.DispatchLink(c, target, actions)
		active.Store(false)
		if handled {
			err = handlerErr
		} else if local {
			err = r.revealScopedLink(tree, target)
		} else if target.Route.Kind == shell.LinkFile && r.attachments != nil && attachmentFileKind(target.Path, "") != conversation.OtherFile {
			r.selectLinkSession(target)
			file := conversation.File{ID: target.Worktree.String() + "/" + target.Path, Path: target.Path}
			r.openCapturedFilePreview(target.Session, file, linkAttachment(target))
		} else {
			key, err = "unavailable", unavailableLink()
		}
	}
	r.reportLink(c, err, key)
}

func (r *Runtime) openLinkWorkspaceFile(target shell.LinkTarget, tree protocol.Worktree, destination shell.LinkFileDestination) error {
	root := r.localRuntime()
	app := root.App
	switch {
	case destination.Panel != "" && destination.Page == "":
		if !target.ConversationWorkspace || !app.Panel.RegisteredResource(destination.Panel) {
			return unavailableLink()
		}
		app.Navigate(shell.ConversationPage)
		project := r.conversationLinkWorkspace(target)
		if project == nil {
			return unavailableLink()
		}
		selected, node := app.Sidebar.Selected, app.Sidebar.SelectedNode()
		project.openFile(target.Path, target.Line, false, func(relative string, line int) {
			if app.Page != shell.ConversationPage || app.Sidebar.Selected != selected || app.Sidebar.SelectedNode() != node ||
				app.ActiveTerminal() != "" || root.selectedRuntime() != r || r.selectedWorkspace() != project {
				return
			}
			project.prepareFileEditor(relative, line)
			app.RevealConversationPanel()
			app.Panel.Show(panel.Tab{Kind: destination.Panel, ID: relative})
		})
	case destination.Page != "" && destination.Panel == "":
		project, err := r.showLinkWorkspacePage(destination.Page, target, tree)
		if err != nil {
			return err
		}
		location := app.ResourceTarget()
		project.openFile(target.Path, target.Line, false, func(relative string, line int) {
			if app.Page == destination.Page && app.ResourceTarget() == location && root.shownResourceWorkspace() == project {
				project.prepareFileEditor(relative, line)
			}
		})
	default:
		return unavailableLink()
	}
	return nil
}

func linkAttachment(target shell.LinkTarget) attachmentDescriptor {
	session, _ := protocol.ParseSessionID(target.Session)
	return attachmentDescriptor{session: session, worktree: target.Worktree, path: target.Path, name: path.Base(target.Path)}
}

func (r *Runtime) revealScopedLink(tree protocol.Worktree, target shell.LinkTarget) error {
	name, kind, err := localFileLinkPath(tree.Path, target.Path)
	if err != nil {
		return err
	}
	if kind != target.Route.Kind {
		return fileActionFault(protocol.ErrorCodeRevisionConflict, "file link target changed")
	}
	return r.openSystemLink(name, false)
}

func localFileLinkPath(root, name string) (string, shell.LinkKind, error) {
	var base string
	var err error
	if root != "" {
		base, err = filepath.EvalSymlinks(root)
		if err != nil {
			return "", "", err
		}
		name = filepath.Join(base, filepath.FromSlash(name))
	} else {
		// Parsed absolute links normalize a drive path to "/C:/…".
		if fileLinkDrive(strings.TrimPrefix(name, "/")) {
			name = strings.TrimPrefix(name, "/")
		}
		name = filepath.FromSlash(name)
	}
	if !filepath.IsAbs(name) {
		return "", "", invalidFileLink()
	}
	resolved, err := filepath.EvalSymlinks(name)
	if err != nil {
		return "", "", err
	}
	if base != "" && resolved != name {
		return "", "", fileActionFault(protocol.ErrorCodePermissionDenied, "file link cannot follow a workspace symlink")
	}
	info, err := os.Stat(resolved)
	if err != nil {
		return "", "", err
	}
	switch {
	case info.IsDir():
		return resolved, shell.LinkDirectory, nil
	case info.Mode().IsRegular():
		return resolved, shell.LinkFile, nil
	default:
		return "", "", invalidFileLink()
	}
}

func (r *Runtime) openSystemLink(name string, external bool) error {
	if r.attachments != nil && r.attachments.native.LocalNode == r.controller.Target() {
		if native := r.attachments.native; external && native.OpenLocal != nil {
			native.OpenLocal(name)
			return nil
		} else if !external && native.RevealLocal != nil {
			native.RevealLocal(name)
			return nil
		}
	}
	return unavailableLink()
}

func (r *Runtime) canOpenLinkInConversation(target shell.LinkTarget) bool {
	root := r.localRuntime()
	session, ok := r.session(target.Session)
	if !ok || session.Worktree != target.Worktree {
		return false
	}
	if root.App.Data.FindOn(target.Node.String(), target.Session) != nil {
		return true
	}
	if root.App.Page == shell.ConversationPage && root.App.ActiveTerminal() == "" && root.selectedRuntime() == r {
		project := r.selectedWorkspace()
		return project != nil && project.owner.ID == target.Worktree
	}
	return false
}

func (r *Runtime) conversationLinkWorkspace(target shell.LinkTarget) *workspaceProject {
	root := r.localRuntime()
	if current, ok := r.session(target.Session); ok && current.Worktree == target.Worktree && r.selectLinkSession(target) {
		r.workspaceFrame(nil)
	}
	if root.selectedRuntime() == r {
		if project := r.selectedWorkspace(); project != nil && project.owner.ID == target.Worktree {
			return project
		}
	}
	return nil
}

func (r *Runtime) selectLinkSession(target shell.LinkTarget) bool {
	root := r.localRuntime()
	app := root.App
	if cv := app.Data.FindOn(target.Node.String(), target.Session); cv != nil {
		if app.SelectedConversation() != cv || app.ActiveTerminal() != "" {
			app.Sidebar.SelectOn(cv.Node, cv.ID)
			root.OpenSession(cv.ID)
		}
		return true
	}
	return false
}

func (r *Runtime) showLinkWorkspacePage(page shell.Page, target shell.LinkTarget, tree protocol.Worktree) (*workspaceProject, error) {
	root := r.localRuntime()
	app := root.App
	if !app.WorkspacePageEnabled(page) {
		return nil, unavailableLink()
	}
	if tree.Project == nil {
		project := r.conversationLinkWorkspace(target)
		if project == nil {
			return nil, unavailableLink()
		}
		app.SetResourceTarget(shell.ResourceTarget{})
		app.Navigate(page)
		return project, nil
	}
	location := shell.ResourceTarget{Node: target.Node.String(), Project: tree.Project.String(), Worktree: tree.ID.String()}
	for _, project := range app.Data.Projects {
		if project.ID == location.Project && project.Host == location.Node && !project.Offline {
			app.SetResourceTarget(location)
			app.Navigate(page)
			root.resourceWorkspaceFrame(root.selectedRuntime())
			if shown := root.shownResourceWorkspace(); shown != nil && shown.runtime == r && shown.owner.ID == tree.ID {
				return shown, nil
			}
			break
		}
	}
	return nil, unavailableLink()
}
