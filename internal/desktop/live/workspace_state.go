package live

import (
	"cmp"
	"context"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// WorkspaceNative delegates native file effects to the window's verified
// transfer owner. Execution-host paths never become controller-local paths.
type WorkspaceNative struct {
	Open     func(protocol.WorktreeID, string, *string)
	Reveal   func(protocol.WorktreeID, string)
	Download func(protocol.WorktreeID, string, *string)
	Insert   func(protocol.WorktreeID, []string)
}
type workspaceState struct {
	projects  map[protocol.WorktreeID]*workspaceProject
	selected  protocol.WorktreeID
	session   protocol.SessionID
	native    WorkspaceNative
	clipboard workspaceClipboard
	admitted  map[protocol.WorktreeID]protocol.Worktree
	transfer  *workspaceTransfer
	transfers map[string]*workspaceTransfer
	uploads   *workspaceUploads
	// savingAll is set from Save all in the close prompt until the frame
	// that starts the saves (closing.go).
	savingAll bool
	// saveAll is that Save all while its frame starts the saves; each save
	// keeps it until it ends.
	saveAll *saveAllBatch
}
type workspaceClipboard struct {
	worktree protocol.WorktreeID
	paths    []string
	cut      bool
	capture  *struct{ marker byte }
}
type workspaceProject struct {
	runtime                                   *Runtime
	owner                                     protocol.Worktree
	session                                   protocol.SessionID
	panel                                     *panel.Project
	data                                      *viewmodel.Workspace
	status                                    protocol.GitStatus
	statusKnown                               bool
	repositoryUnavailable                     bool
	repositoryNotice                          string
	gitVisible                                bool
	git                                       panel.GitWorkspaceBinding
	worktrees                                 protocol.GitWorktrees
	locationsLoaded                           bool
	checkpointSessions                        map[protocol.SessionID]*workspaceCheckpoints
	reviewTurns                               map[string]protocol.TurnID
	turnView                                  *protocol.TurnID
	reviewTarget                              *workspaceTurnReview
	files                                     map[string]protocol.FileContent
	legacyFiles                               map[legacyFileScope]*legacyFileWorkspace
	changedFiles                              map[string]changedFile
	loading                                   map[string]bool
	failedReads                               map[string]bool
	missingEntries                            map[string]bool
	staleReads                                map[string]bool
	trashing                                  map[string]bool
	readReported                              map[string]string // read key -> toast key of its reported failure
	resourceSignature                         string
	resourceOlder                             bool
	restoreBatches                            map[string]*fileBatch
	uncertainOperations                       map[string]protocol.Command
	uncertainCallbacks                        map[string]func(*ui.Context, protocol.Output)
	searchCancel                              context.CancelFunc
	searchGeneration                          uint64
	openingFile                               *workspaceFileOpen
	fileDirectoryRevision, fileDirectoryFence uint64
	fileDirectoryCancel                       context.CancelFunc
	filePicker                                filePicker
	documents                                 map[string]uint32
	documentImages                            map[string]*documentImages
	artifacts                                 map[string]protocol.ArtifactSummary
	hunks                                     map[string]protocol.ConversationCheckpointHunkReview
	initialized                               bool
	pendingFiles, pendingGit                  bool
}

func (r *Runtime) initWorkspace() {
	r.workspace = &workspaceState{projects: map[protocol.WorktreeID]*workspaceProject{}, admitted: map[protocol.WorktreeID]protocol.Worktree{}}
}
func (r *Runtime) SetWorkspaceNative(native WorkspaceNative) {
	if r.workspace == nil {
		r.initWorkspace()
	}
	r.workspace.native = native
	for _, bound := range r.bindings {
		bound.SetWorkspaceNative(native)
	}
}
func (r *Runtime) workspaceProject() *panel.Project {
	if r.parent == nil {
		if owner := r.selectedRuntime(); owner == nil {
			return nil
		} else if owner != r {
			return owner.workspaceProject()
		}
	}
	project := r.selectedWorkspace()
	if project == nil {
		return nil
	}
	return project.panel
}

func (r *Runtime) selectedWorkspace() *workspaceProject {
	if r == nil || r.workspace == nil {
		return nil
	}
	return r.workspace.projects[r.workspace.selected]
}

func (r *Runtime) workspaceWorktree(id protocol.WorktreeID) (protocol.Worktree, bool) {
	for _, tree := range r.snapshot().Worktrees {
		if tree.ID == id {
			delete(r.workspace.admitted, id)
			return tree, true
		}
	}
	tree, found := r.workspace.admitted[id]
	return tree, found
}

func (r *Runtime) workspaceFrame(c *ui.Context) {
	r.bindArtifactCallbacks()
	if r.workspace == nil {
		r.initWorkspace()
	}
	for _, project := range r.workspace.projects {
		project.pruneClosedContent()
	}
	r.projectWorkspaceLocations()
	var session protocol.Session
	ok := false
	pane := r.App.ActiveWorkspacePane()
	var info protocol.TerminalInfo
	if pane.Kind == "" {
		session, ok = r.session(r.App.Sidebar.Selected)
		// An unsent draft has a worktree to show and no session.
		r.App.Panel.Session = ok
		if !ok {
			session, ok = r.draftWorkspaceSession(r.App.Sidebar.Selected)
		}
	} else {
		r.App.Panel.Session, r.App.Panel.HasProject, r.App.Panel.Cwd = false, false, ""
		if pane.Kind == panes.Terminal {
			// The terminal's worktree is independent of the selected conversation.
			var owner *Runtime
			owner, info, ok = r.liveTerminalInfo(pane.ID)
			ok = ok && owner == r && info.Worktree != nil
			if ok {
				session.Worktree = *info.Worktree
			}
		}
	}
	if !ok {
		r.clearWorkspaceSelection()
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
		return
	}
	worktree, found := r.workspaceWorktree(session.Worktree)
	if found && pane.Kind == panes.Terminal {
		session.Project = worktree.Project
		r.App.Panel.Cwd = cmp.Or(workspaceString(info.Directory), worktree.Path)
	}
	if !found {
		r.clearWorkspaceSelection()
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
		return
	}
	changed := r.workspace.selected != session.Worktree || r.workspace.session != session.ID
	if r.workspace.selected != session.Worktree {
		r.releaseSelectedWorkspace()
	}
	r.workspace.selected, r.workspace.session = session.Worktree, session.ID
	project := r.workspace.projects[worktree.ID]
	if project == nil {
		project = r.workspaceFor(worktree)
	}
	if changed || project.session != session.ID {
		project.cancelFileOpen()
		project.session = session.ID
		project.data.Checkpoints = nil
		if checkpoints := project.checkpointSessions[session.ID]; checkpoints != nil {
			project.data.Checkpoints = checkpoints.history
		}
		project.data.Documents = nil
		project.documents = map[string]uint32{}
		project.hunks = map[string]protocol.ConversationCheckpointHunkReview{}
		project.resourceSignature = ""
		project.resourceOlder = false
		project.reviewTurns = map[string]protocol.TurnID{}
		project.turnView = nil
		project.reviewTarget = nil
		project.readSessionResources()
	}
	project.owner = worktree
	r.App.Panel.Data = project.data
	r.App.Panel.Actions = project.panel.Actions
	r.App.Panel.HasProject = session.Project != nil
	if !r.controller.Connected() {
		project.releaseGitView()
		return
	}
	project.resourceFrame()
	project.sessionResourcesFrame()
}

func (p *workspaceProject) resourceFrame() {
	if p.pendingFiles || !p.initialized {
		busy := len(p.trashing) != 0
		for key := range p.loading {
			if strings.HasPrefix(key, "file/") || strings.HasPrefix(key, "file-check/") {
				busy = true
				break
			}
		}
		if !busy {
			p.pendingFiles = false
			p.initialized = true
			p.refreshFiles()
		}
	}
	root := p.runtime.localRuntime()
	if root.App.Page == shell.ConversationPage && !root.App.DemoOn() || root.App.ResourceKind() == panel.Changes && root.shownResourceWorkspace() == p {
		p.gitFrame()
	} else if p.gitVisible {
		p.releaseGitView()
	}
}
func (r *Runtime) workspaceFor(worktree protocol.Worktree) *workspaceProject {
	if project := r.workspace.projects[worktree.ID]; project != nil {
		return project
	}
	project := &workspaceProject{runtime: r, owner: worktree, data: &viewmodel.Workspace{}, files: map[string]protocol.FileContent{}, loading: map[string]bool{}, documents: map[string]uint32{}, artifacts: map[string]protocol.ArtifactSummary{}, hunks: map[string]protocol.ConversationCheckpointHunkReview{}, reviewTurns: map[string]protocol.TurnID{}}
	project.panel = panel.NewLiveProject(worktree.ID.String(), project.data, project.actions())
	project.panel.DocumentsLocked = r.documentsLocked
	project.panel.FileDirectory = project.fileDirectoryHost()
	project.bindGitWorkspace()
	r.workspace.projects[worktree.ID] = project
	return project
}
func (r *Runtime) releaseSelectedWorkspace() {
	if old := r.selectedWorkspace(); old != nil && old != r.localRuntime().resourcePage.shown {
		old.releaseView()
	}
}

func (r *Runtime) clearWorkspaceSelection() {
	if r.workspace != nil {
		r.releaseSelectedWorkspace()
		r.workspace.selected, r.workspace.session = protocol.WorktreeID{}, protocol.SessionID{}
	}
}

func (p *workspaceProject) selectedSession(session protocol.SessionID) bool {
	return p.runtime.selectedWorkspace() == p && p.session == session
}

func (p *workspaceProject) releaseView() {
	p.releaseResourceWatch()
	p.filePicker.close()
	p.cancelFileSearch()
	p.panel.SuspendFilePreviews()
	p.cancelFileOpen()
	p.pruneClosedContent()
}

func (p *workspaceProject) releaseResourceWatch() {
	p.pendingFiles, p.pendingGit = false, false
	p.releaseGitView()
}

// pruneClosedContent keeps the workspace's navigation state while dropping
// file bodies and patches that no open editor or diff needs. A later read
// recovers them from the Node with the then-current revision.
func (p *workspaceProject) pruneClosedContent() {
	p.pruneDocumentImages()
	for path := range p.files {
		if p.panel.Git.UsesFileDiff(path) || !p.panel.ReleaseClosedFile(path) {
			continue
		}
		delete(p.files, path)
		delete(p.changedFiles, path)
		delete(p.failedReads, "file/"+path)
	}
	if p.git.Prune != nil {
		p.git.Prune()
	}
	for path := range p.hunks {
		if !p.panel.Git.UsesDiff(path) {
			delete(p.hunks, path)
		}
	}

}
func (r *Runtime) closeWorkspace() {
	if r.workspace == nil {
		return
	}
	r.closeWorkspaceUploads()
	for _, project := range r.workspace.projects {
		project.closeDocumentImages()
		project.releaseResourceWatch()
		project.closeFileDirectory()
		project.panel.SuspendFilePreviews()
		project.cancelFileOpen()
	}
}
func (p *workspaceProject) actions() *panel.Actions {
	return &panel.Actions{
		ReadFile: p.readFile, OpenFile: func(path string) bool { p.openWorkspaceFile(path, 0); return true }, ReadNotebook: p.readEditorNotebook, SaveFile: p.saveFile, File: p.fileAction, ReadDiff: p.readDiff, ReadDocument: p.readDocument, Restore: p.restore, Refresh: p.refresh,
		DocumentMarkdown: p.documentMarkdown,
		Search:           p.searchFiles, CancelSearch: p.cancelFileSearch, OpenFileAt: p.openWorkspaceFile, OpenManagedFile: p.openManagedFile, Pending: p.pendingWorkspaceOperations,
		Loading:    p.resourcesLoading,
		Upload:     func(directory string) { p.runtime.UploadWorkspaceFiles(p.owner.ID, directory) },
		PasteReady: func() bool { return len(p.runtime.workspace.clipboard.paths) > 0 },
	}
}
func (p *workspaceProject) read(key string, command protocol.Command, done func(*ui.Context, protocol.Output), failures ...func(*ui.Context, error)) {
	if p.loading[key] || p.failedReads[key] || p.trashBlocksRead(key) {
		return
	}
	p.loading[key] = true
	p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/"+key, command, func(c *ui.Context, out protocol.Output, err error) {
		delete(p.loading, key)
		if p.staleReads[key] {
			delete(p.staleReads, key)
			return
		}
		if err != nil {
			if strings.HasPrefix(key, "file/") {
				relative := strings.TrimPrefix(key, "file/")
				if faultCode(err) == protocol.ErrorCodeNotFound {
					p.entryMissing(relative)
				}
				p.fileUnavailable(relative)
			}
			if p.failedReads == nil {
				p.failedReads = map[string]bool{}
			}
			p.failedReads[key] = true
			// The read's command names the failure; keys can hold paths.
			if outcome, name, own := workspaceReadFailure(key, err); own {
				kit.RecordFailure("desktop", command.Kind, outcome, err)
				p.reportRead(c, key, outcome, name)
			} else {
				outcome, shown := outcomeKey(err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				kit.RecordFailure("desktop", command.Kind, outcome, err)
				kit.Toast(c, outcome, i18n.T(c, shown))
			}
			for _, failure := range failures {
				failure(c, err)
			}
			return
		}
		delete(p.readReported, key)
		done(c, out)
	})
}

// reportRead toasts a failed folder or Git read once per cause: the reads a
// watch repeats, and the Git status and worktree list that fail together,
// tell it once until the read succeeds or the user refreshes. A project's
// worktree list read for location labels while another project is shown
// leaves its failure to the read made when that project is shown.
func (p *workspaceProject) reportRead(c *ui.Context, key string, outcome kit.Outcome, name string) {
	group := workspaceReadGroup(key)
	shown := p.runtime.selectedWorkspace()
	reports := p.readReported
	var locationReport string
	if key == "git-worktrees" {
		if shown == nil || shown.locationWorkspace() != p {
			return
		}
		// Location reads use the main checkout, even when a linked checkout
		// owns the repository status shown in the conversation.
		reports, locationReport = shown.readReported, p.readReported[key]
	} else if group == "locations" && shown == p {
		locationReport = p.locationWorkspace().readReported["git-worktrees"]
	}
	told := locationReport == name || key == "git-worktrees" && shown != nil && shown.repositoryNotice == name
	for other, reported := range reports {
		if reported == name && workspaceReadGroup(other) == group {
			told = true
			break
		}
	}
	if p.readReported == nil {
		p.readReported = map[string]string{}
	}
	p.readReported[key] = name
	if !told {
		kit.Toast(c, outcome, i18n.T(c, name))
	}
}

func (p *workspaceProject) locationWorkspace() *workspaceProject {
	if p.owner.Project != nil && !p.owner.Main {
		for _, location := range p.runtime.workspace.projects {
			if location.owner.Main && location.owner.Project != nil && *location.owner.Project == *p.owner.Project {
				return location
			}
		}
	}
	return p
}

func (p *workspaceProject) refresh() {
	if p.runtime.controller.Connected() {
		p.closeDocumentImages()
	}
	if location := p.locationWorkspace(); location != p {
		delete(location.readReported, "git-worktrees")
		delete(location.failedReads, "git-worktrees")
	}
	p.readReported = nil
	p.repositoryNotice = ""
	p.turnView = nil
	p.reviewTarget = nil
	p.reviewTurns = map[string]protocol.TurnID{}
	p.failedReads = map[string]bool{}
	p.missingEntries = nil
	p.documents = map[string]uint32{}
	p.refreshFiles()
	p.readGit()

	if checkpoints := p.checkpointSessions[p.session]; checkpoints != nil {
		clear(checkpoints.runs)
	}
	p.resourceOlder = false
	p.readSessionResources()
}
func (p *workspaceProject) readFile(relative string) {
	if relative == "" || p.trashBlocksRead("file/"+relative) {
		return
	}
	if p.entryIsMissing(relative) {
		p.settleChanged(relative)
		return
	}
	if value, loaded := p.files[relative]; loaded {
		// A tab closed and opened again is a new view of the file already
		// read: it shows that content and is as writable as it.
		if !p.panel.FileLoaded(relative) {
			p.panel.SetFile(relative, value.Text)
			p.panel.SetFileWritable(relative, value.Revision != nil && !value.Truncated)
			p.panel.SetFileStatus(relative, false, value.Truncated)
		}
		p.settleChanged(relative)
		return
	}
	p.loadFile(relative)
}

// loadFile reads a file into its editor. A file that is open keeps its source
// until the read ends, so a draft begun meanwhile still names the content it
// began from.
func (p *workspaceProject) loadFile(relative string) {
	p.read("file/"+relative, protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: p.owner.ID, Path: relative}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.FileContent](out.Data)
		if !ok {
			p.fileUnavailable(relative)
			return
		}
		p.fileLoaded(relative, value)
	})
}

// fileLoaded shows the content a read returned. Under a draft it is not the
// draft's new source but what the file holds on disk now.
func (p *workspaceProject) fileLoaded(relative string, value protocol.FileContent) {
	p.entryFound(relative)
	if _, loaded := p.files[relative]; loaded {
		if _, dirty := p.panel.FileDraft(relative); dirty {
			p.draftSource(relative, changedFile{content: value})
			return
		}
	}
	p.checkDocumentImagesRevision(relative, value.Revision)
	p.files[relative] = value
	p.clearChanged(relative)
	p.panel.SetFile(relative, value.Text)
	p.panel.SetFileUnavailable(relative, false)
	p.panel.SetFileWritable(relative, value.Revision != nil && !value.Truncated)
	p.panel.SetFileStatus(relative, false, value.Truncated)
}
func (p *workspaceProject) fileUnavailable(relative string) {
	_, dirty := p.panel.FileDraft(relative)
	if _, loaded := p.files[relative]; loaded {
		if dirty {
			// The draft keeps its source; the next refresh tells what became
			// of the file.
			return
		}
		delete(p.files, relative)
	}
	p.panel.SetFileUnavailable(relative, true)
	p.panel.SetFileStatus(relative, false, false)
}
func (p *workspaceProject) refreshFiles() {
	if len(p.trashing) != 0 {
		p.pendingFiles = true
		return
	}
	p.fileDirectoryRevision++
	for key := range p.failedReads {
		if relative, file := strings.CutPrefix(key, "file/"); file && !p.entryIsMissing(relative) {
			delete(p.failedReads, key)
		}
	}
	for filename := range p.files {
		if p.entryIsMissing(filename) {
			continue
		}
		if _, dirty := p.panel.FileDraft(filename); dirty {
			p.checkDraft("draft", filename, nil)
		} else {
			p.loadFile(filename)
		}
	}
}
