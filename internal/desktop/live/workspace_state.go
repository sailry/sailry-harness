package live

import (
	"context"
	"path"
	"sort"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
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
}
type workspaceClipboard struct {
	worktree protocol.WorktreeID
	paths    []string
	cut      bool
}
type workspaceProject struct {
	runtime                  *Runtime
	owner                    protocol.Worktree
	session                  protocol.SessionID
	panel                    *panel.Project
	data                     *viewmodel.Workspace
	status                   protocol.GitStatus
	statusKnown              bool
	worktrees                protocol.GitWorktrees
	locationsLoaded          bool
	turnChanges              map[string][]viewmodel.FileChange
	turnUndone               map[string]map[string]bool
	reviewTurns              map[string]protocol.TurnID
	turnView                 *protocol.TurnID
	branches                 protocol.GitBranches
	gitLogPages              []protocol.GitLog
	gitLogPage               int
	gitLastRead              time.Time
	gitRefreshTimer          *time.Timer
	gitRefreshGeneration     uint64
	files                    map[string]protocol.FileContent
	changedFiles             map[string]changedFile
	directories              map[string]bool
	loading                  map[string]bool
	failedReads              map[string]bool
	diffs                    map[string]bool
	diffContents             map[string][]viewmodel.Hunk
	manualDiffs              map[string]workspaceGitDiff
	resourceSignature        string
	resourceRuns             map[protocol.TurnID]protocol.ConversationStatus
	resourceOlder            bool
	restoreBatches           map[string]*fileBatch
	uncertainOperations      map[string]protocol.Command
	uncertainCallbacks       map[string]func(*ui.Context, protocol.Output)
	searchCancel             context.CancelFunc
	searchGeneration         uint64
	directoryPages           map[string]protocol.Directory
	directoryEntries         map[string][]protocol.FileEntry
	directoryStates          map[string]panel.DirectoryState
	checkpoints              map[string]protocol.ConversationCheckpointFile
	checkpointOrder          map[string][]protocol.ConversationCheckpointFile
	documents                map[string]uint32
	artifacts                map[string]protocol.ArtifactSummary
	hunks                    map[string]protocol.ConversationCheckpointHunkReview
	cancel                   context.CancelFunc
	release                  func()
	initialized              bool
	pendingFiles, pendingGit bool
	gitRunning               int // Git actions sent and not yet answered
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
	if r.workspace == nil {
		return nil
	}
	project := r.workspace.projects[r.workspace.selected]
	if project == nil {
		return nil
	}
	return project.panel
}
func (r *Runtime) workspaceFrame(c *ui.Context) {
	r.bindArtifactCallbacks()
	if r.workspace == nil {
		r.initWorkspace()
	}
	r.projectWorkspaceLocations()
	session, ok := r.session(r.App.Sidebar.Selected)
	// An unsent draft has a worktree to show and no session.
	r.App.Panel.Session = ok
	if !ok {
		session, ok = r.draftWorkspaceSession(r.App.Sidebar.Selected)
	}
	if !ok {
		r.releaseSelectedWorkspace()
		r.workspace.selected = protocol.WorktreeID{}
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
		return
	}
	var worktree protocol.Worktree
	found := false
	for _, candidate := range r.snapshot().Worktrees {
		if candidate.ID == session.Worktree {
			worktree = candidate
			found = true
			break
		}
	}
	if !found {
		if admitted, ok := r.workspace.admitted[session.Worktree]; ok {
			worktree, found = admitted, true
		}
	}
	if !found {
		r.releaseSelectedWorkspace()
		r.workspace.selected = protocol.WorktreeID{}
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
		return
	}
	if r.workspace.selected != session.Worktree {
		r.releaseSelectedWorkspace()
	}
	r.workspace.selected, r.workspace.session = session.Worktree, session.ID
	project := r.workspace.projects[worktree.ID]
	if project == nil {
		project = r.workspaceFor(worktree)
	}
	if project.session != session.ID {
		project.session = session.ID
		project.data.Checkpoints = nil
		project.data.Documents = nil
		project.checkpoints = map[string]protocol.ConversationCheckpointFile{}
		project.checkpointOrder = map[string][]protocol.ConversationCheckpointFile{}
		project.documents = map[string]uint32{}
		project.hunks = map[string]protocol.ConversationCheckpointHunkReview{}
		project.resourceSignature = ""
		project.turnChanges = map[string][]viewmodel.FileChange{}
		project.turnUndone = map[string]map[string]bool{}
		project.reviewTurns = map[string]protocol.TurnID{}
		project.turnView = nil
		project.readSessionResources()
	}
	project.owner = worktree
	r.App.Panel.Data = project.data
	r.App.Panel.Actions = project.panel.Actions
	r.App.Panel.HasProject = session.Project != nil
	if project.release == nil {
		project.watch()
	}
	if project.pendingFiles {
		busy := false
		for key := range project.loading {
			if strings.HasPrefix(key, "file/") || strings.HasPrefix(key, "directory") {
				busy = true
			}
		}
		if !busy {
			project.pendingFiles = false
			project.refreshFiles()
		}
	}
	if project.pendingGit && !project.loading["git-status"] && !project.loading["git-branches"] && !project.loading["git-log"] {
		project.refreshWatchedGit()
	}
	if !project.initialized {
		project.initialized = true
		project.refresh()
	}
	project.sessionResourcesFrame()
}
func (r *Runtime) workspaceFor(worktree protocol.Worktree) *workspaceProject {
	if project := r.workspace.projects[worktree.ID]; project != nil {
		return project
	}
	project := &workspaceProject{runtime: r, owner: worktree, data: &viewmodel.Workspace{Files: &viewmodel.FileNode{Name: path.Base(worktree.Path), Children: []*viewmodel.FileNode{}}}, files: map[string]protocol.FileContent{}, directories: map[string]bool{}, loading: map[string]bool{}, diffs: map[string]bool{}, checkpoints: map[string]protocol.ConversationCheckpointFile{}, checkpointOrder: map[string][]protocol.ConversationCheckpointFile{}, documents: map[string]uint32{}, artifacts: map[string]protocol.ArtifactSummary{}, hunks: map[string]protocol.ConversationCheckpointHunkReview{}, turnChanges: map[string][]viewmodel.FileChange{}, turnUndone: map[string]map[string]bool{}, reviewTurns: map[string]protocol.TurnID{}}
	project.panel = panel.NewLiveProject(worktree.ID.String(), project.data, project.actions())
	r.workspace.projects[worktree.ID] = project
	return project
}
func (r *Runtime) releaseSelectedWorkspace() {
	if old := r.workspace.projects[r.workspace.selected]; old != nil {
		if old.release != nil {
			old.release()
			old.release = nil
		}
		if old.cancel != nil {
			old.cancel()
			old.cancel = nil
		}
		old.panel.ResetFileSearch()
		old.stopGitRefreshTimer()
	}
}
func (r *Runtime) closeWorkspace() {
	if r.workspace == nil {
		return
	}
	r.closeWorkspaceUploads()
	for _, project := range r.workspace.projects {
		project.stopGitRefreshTimer()
		project.panel.ResetFileSearch()
		if project.cancel != nil {
			project.cancel()
		}
		if project.release != nil {
			project.release()
		}
	}
}
func (p *workspaceProject) actions() *panel.Actions {
	return &panel.Actions{
		ReadDirectory: func(path string) { p.readDirectory(path, false) }, NextDirectory: p.nextDirectory, RefreshDirectory: func(path string) { p.readDirectory(path, true) },
		ReadFile: p.readFile, OpenFile: p.openFileKind, ReadNotebook: p.readEditorNotebook, SaveFile: p.saveFile, File: p.fileAction, Git: p.gitAction, ReadDiff: p.readDiff, ReadCommit: p.openGitCommit, ReadDocument: p.readDocument, Hunk: p.hunk, Restore: p.restore, Refresh: p.refresh,
		Search: p.searchFiles, CancelSearch: p.cancelFileSearch, OpenFileAt: p.openFileAt, Pending: p.pendingWorkspaceOperations,
		GitHistory: p.gitHistoryPage,
		PendingGit: p.pendingGitOperations,
		Loading:    p.resourcesLoading,
		GitBusy:    func() bool { return p.gitRunning > 0 },
		Upload:     func(directory string) { p.runtime.UploadWorkspaceFiles(p.owner.ID, directory) },
		PasteReady: func() bool { return len(p.runtime.workspace.clipboard.paths) > 0 },
	}
}
func (p *workspaceProject) read(key string, command protocol.Command, done func(*ui.Context, protocol.Output), failures ...func(*ui.Context, error)) {
	if p.loading[key] || p.failedReads[key] {
		return
	}
	p.loading[key] = true
	p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/"+key, command, func(c *ui.Context, out protocol.Output, err error) {
		delete(p.loading, key)
		if err != nil {
			if key == "git-log" {
				p.projectGitHistory(false)
			}
			if strings.HasPrefix(key, "file/") {
				p.fileUnavailable(strings.TrimPrefix(key, "file/"))
			}
			if key == "git-status" {
				p.statusKnown = false
				p.panel.SetGitState(panel.GitState{})
			}
			if p.failedReads == nil {
				p.failedReads = map[string]bool{}
			}
			p.failedReads[key] = true
			if strings.HasPrefix(key, "directory/") {
				directory := strings.TrimPrefix(key, "directory/")
				state := p.directoryStates[directory]
				state.Loading = false
				state.Failed = true
				state.Next = false
				p.directoryState(directory, state)
			}
			p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			for _, failure := range failures {
				failure(c, err)
			}
			return
		}
		done(c, out)
	})
}
func (p *workspaceProject) refresh() {
	p.turnView = nil
	p.failedReads = map[string]bool{}
	p.documents = map[string]uint32{}
	p.diffs = map[string]bool{}
	p.refreshFiles()
	p.readGit()

	p.readSessionResources()
}
func (p *workspaceProject) readDirectory(directory string, refresh bool) {
	if p.directories[directory] && !refresh {
		return
	}
	if refresh {
		delete(p.failedReads, "directory/"+directory)
	}
	p.requestDirectory(directory, nil, nil, len(p.directoryEntries[directory]), false)
}
func (p *workspaceProject) nextDirectory(directory string) {
	page := p.directoryPages[directory]
	if page.Next == nil {
		return
	}
	delete(p.failedReads, "directory/"+directory)
	p.requestDirectory(directory, page.Next, append([]protocol.FileEntry(nil), p.directoryEntries[directory]...), 0, p.directoryStates[directory].Partial)
}
func (p *workspaceProject) directoryState(directory string, state panel.DirectoryState) {
	if p.directoryStates == nil {
		p.directoryStates = map[string]panel.DirectoryState{}
	}
	p.directoryStates[directory] = state
	p.panel.SetDirectoryState(directory, state)
}
func (p *workspaceProject) requestDirectory(directory string, after *protocol.DirectoryCursor, accumulated []protocol.FileEntry, extent int, partial bool) {
	key := "directory/" + directory
	if p.loading[key] || p.failedReads[key] {
		return
	}
	state := p.directoryStates[directory]
	state.Loading = true
	state.Failed = false
	p.directoryState(directory, state)
	p.read(key, protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: p.owner.ID, Path: directory, After: after}}, func(c *ui.Context, out protocol.Output) {
		page, ok := detail[protocol.Directory](out.Data)
		if !ok {
			state := p.directoryStates[directory]
			state.Loading = false
			state.Failed = true
			p.directoryState(directory, state)
			return
		}
		accumulated = append(accumulated, page.Entries...)
		partial = partial || page.UnsupportedNames > 0 || page.Truncated && page.Next == nil
		if page.Next != nil && len(accumulated) < extent {
			p.requestDirectory(directory, page.Next, accumulated, extent, partial)
			return
		}
		if p.directoryPages == nil {
			p.directoryPages = map[string]protocol.Directory{}
			p.directoryEntries = map[string][]protocol.FileEntry{}
		}
		p.directoryPages[directory] = page
		p.directoryPage(c, directory, page, accumulated)
		p.directoryState(directory, panel.DirectoryState{Loaded: true, Partial: partial, Next: page.Next != nil})
	}, func(c *ui.Context, err error) {
		if after != nil && faultCode(err) == protocol.ErrorCodeRevisionConflict {
			p.readDirectory(directory, true)
		}
	})
}
func (p *workspaceProject) directoryPage(c *ui.Context, directory string, page protocol.Directory, accumulated []protocol.FileEntry) {
	if p.directoryEntries == nil {
		p.directoryEntries = map[string][]protocol.FileEntry{}
	}
	node := workspaceFileNode(p.data.Files, directory)
	if node == nil {
		return
	}
	old := map[string]*viewmodel.FileNode{}
	for _, child := range node.Children {
		old[child.Path] = child
	}
	names := map[string]bool{}
	entries := []protocol.FileEntry{}
	children := []*viewmodel.FileNode{}
	for _, entry := range accumulated {
		if names[entry.Name] {
			continue
		}
		names[entry.Name] = true
		entries = append(entries, entry)
		relative := path.Join(directory, entry.Name)
		child := old[relative]
		if child == nil {
			child = &viewmodel.FileNode{Name: entry.Name, Path: relative}
		}
		if entry.Kind == protocol.EntryKindDirectory {
			if child.Children == nil {
				child.Children = []*viewmodel.FileNode{}
			}
		} else {
			child.Children = nil
		}
		children = append(children, child)
	}
	node.Children = children
	p.panel.PruneFileSelection()
	p.directories[directory] = true
	p.directoryEntries[directory] = entries
}
func workspaceFileNode(root *viewmodel.FileNode, relative string) *viewmodel.FileNode {
	if root == nil {
		return nil
	}
	if root.Path == relative {
		return root
	}
	for _, child := range root.Children {
		if found := workspaceFileNode(child, relative); found != nil {
			return found
		}
	}
	return nil
}
func (p *workspaceProject) readFile(relative string) {
	if relative == "" {
		return
	}
	if _, loaded := p.files[relative]; loaded {
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
	if _, loaded := p.files[relative]; loaded {
		if _, dirty := p.panel.FileDraft(relative); dirty {
			p.draftSource(relative, changedFile{content: value})
			return
		}
	}
	p.files[relative] = value
	p.clearChanged(relative)
	p.ensureFile(relative)
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
	if !dirty {
		if node := workspaceFileNode(p.data.Files, relative); node != nil {
			node.Text = ""
		}
	}
}
func (p *workspaceProject) ensureFile(relative string) {
	node := p.data.Files
	parts := strings.Split(relative, "/")
	for index, name := range parts {
		childPath := strings.Join(parts[:index+1], "/")
		var child *viewmodel.FileNode
		for _, candidate := range node.Children {
			if candidate.Path == childPath {
				child = candidate
				break
			}
		}
		if child == nil {
			child = &viewmodel.FileNode{Name: name, Path: childPath}
			if index < len(parts)-1 {
				child.Children = []*viewmodel.FileNode{}
			}
			node.Children = append(node.Children, child)
		}
		node = child
	}
}
func (p *workspaceProject) readGit() {
	p.gitLastRead = time.Now()
	for _, project := range p.runtime.workspace.projects {
		if project.owner.Project != nil && p.owner.Project != nil && *project.owner.Project == *p.owner.Project {
			project.locationsLoaded = false
		}
	}
	for key := range p.failedReads {
		if strings.HasPrefix(key, "git-") || strings.HasPrefix(key, "diff/") || strings.HasPrefix(key, "hunks/") {
			delete(p.failedReads, key)
		}
	}
	p.diffs = map[string]bool{}
	p.read("git-status", protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: p.owner.ID}}, func(c *ui.Context, out protocol.Output) {
		if value, ok := detail[protocol.GitStatus](out.Data); ok {
			p.status = value
			p.statusKnown = true
			p.projectStatus(value)
			if value.Kind != protocol.RepositoryKindDirectory {
				p.readGitHistory()
			} else {
				p.data.Repo = viewmodel.Repository{}
			}
		}
	})
}
func (p *workspaceProject) readGitHistory() {
	p.read("git-branches", protocol.Command{Kind: "list_git_branches", Data: protocol.CommandListGitBranchesPayload{Worktree: p.owner.ID}}, func(c *ui.Context, out protocol.Output) {
		if value, ok := detail[protocol.GitBranches](out.Data); ok {
			p.branches = value
			p.projectBranches(value)
		}
	})
	p.readGitLog(nil, true)
}
func (p *workspaceProject) projectStatus(value protocol.GitStatus) {
	conflicts := map[string]bool{}
	for _, entry := range value.Entries {
		if entry.Conflicted {
			conflicts[entry.Path] = true
		}
	}
	p.panel.SetGitState(panel.GitState{Known: true, Kind: string(value.Kind), Repositories: append([]string(nil), value.Repositories...), Conflicts: conflicts, Head: value.Head != nil, HeadMessage: workspaceString(p.branches.Sync.HeadMessage)})
	if p.turnView != nil {
		return
	}
	old := map[string][]viewmodel.Hunk{}
	for _, change := range p.data.Changes {
		old[change.Path] = change.Hunks
	}
	p.data.Changes = []viewmodel.FileChange{}
	for _, entry := range value.Entries {
		change := viewmodel.FileChange{Path: entry.Path, Staged: workspaceChange(entry.Staged), Unstaged: workspaceChange(entry.Unstaged), Untracked: entry.Untracked, Added: int(entry.Diff.Additions), Removed: int(entry.Diff.Deletions), Hunks: old[entry.Path]}
		if entry.Conflicted {
			// An unmerged path is a change still to be staged.
			change.Unstaged = "U"
		}
		change.Status = change.Unstaged
		if change.Status == "" {
			change.Status = change.Staged
		}
		if entry.Untracked {
			change.Status = "A"
		}
		p.data.Changes = append(p.data.Changes, change)
	}
	if value.Branch != nil {
		p.data.Repo.Branch = *value.Branch
	} else {
		p.data.Repo.Branch = ""
	}
	if p.panel.Git.Change() == "" && len(p.data.Changes) > 0 {
		// The change a host shows first. Its diff opens with the host: a
		// status read opens no tab, and a closed diff stays closed.
		p.panel.Git.First(p.data.Changes[0].Path)
	}
}
func workspaceChange(kind *protocol.GitChangeKind) string {
	if kind == nil {
		return ""
	}
	switch *kind {
	case protocol.GitChangeKindAdded:
		return "A"
	case protocol.GitChangeKindDeleted:
		return "D"
	case protocol.GitChangeKindRenamed:
		return "R"
	case protocol.GitChangeKindTypeChanged:
		return "T"
	}
	return "M"
}
func (p *workspaceProject) projectBranches(value protocol.GitBranches) {
	p.panel.SetGitHeadMessage(workspaceString(value.Sync.HeadMessage))
	repo := &p.data.Repo
	repo.Branches = nil
	repo.Remotes = nil
	repo.Tags = nil
	repo.Stashes = nil
	if value.Current != nil {
		repo.Branch = *value.Current
	} else {
		repo.Branch = ""
	}
	if value.Sync.Upstream != nil {
		repo.Upstream = *value.Sync.Upstream
	} else {
		repo.Upstream = ""
	}
	for _, branch := range value.Entries {
		repo.Branches = append(repo.Branches, viewmodel.Branch{Name: branch.Name, Commit: branch.Commit, Remote: branch.Remote, Current: branch.Current, Tracking: value.Sync.Tracking[branch.Name]})
	}
	names := []string{}
	for name := range value.Sync.RemoteUrls {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		repo.Remotes = append(repo.Remotes, viewmodel.Remote{Name: name, URL: value.Sync.RemoteUrls[name]})
	}
	for _, tag := range value.Sync.Tags {
		repo.Tags = append(repo.Tags, tag.Name)
	}
	for _, stash := range value.Sync.Stashes {
		repo.Stashes = append(repo.Stashes, stash.Message)
	}
	stashes := []string{}
	for _, stash := range value.Sync.Stashes {
		stashes = append(stashes, stash.Commit)
	}
	tags := map[string]string{}
	for _, tag := range value.Sync.Tags {
		tags[tag.Name] = tag.Commit
	}
	p.panel.SetGitTargets(stashes, tags)
	p.panel.SetGitOperation(value.Sync.Operation != nil)
}

func (p *workspaceProject) refreshFiles() {
	for key := range p.failedReads {
		if strings.HasPrefix(key, "file/") || strings.HasPrefix(key, "directory") {
			delete(p.failedReads, key)
		}
	}
	directories := []string{""}
	for directory := range p.directories {
		if directory != "" {
			if workspaceFileNode(p.data.Files, directory) == nil {
				delete(p.directories, directory)
				continue
			}
			directories = append(directories, directory)
		}
	}
	for _, directory := range directories {
		p.readDirectory(directory, true)
	}
	for filename := range p.files {
		if _, dirty := p.panel.FileDraft(filename); dirty {
			p.checkDraft("draft", filename, nil)
		} else {
			p.loadFile(filename)
		}
	}
}
func (p *workspaceProject) watch() {
	ctx, cancel := context.WithCancel(p.runtime.ctx)
	p.cancel = cancel
	files, err := p.runtime.controller.ObserveWorkspace(ctx, p.owner.ID, false, func(protocol.Update) { p.runtime.enqueue(func(*ui.Context) { p.pendingFiles = true }) })
	if err != nil {
		cancel()
		return
	}
	repository, err := p.runtime.controller.ObserveWorkspace(ctx, p.owner.ID, true, func(protocol.Update) { p.runtime.enqueue(func(*ui.Context) { p.pendingGit = true }) })
	if err != nil {
		files()
		cancel()
		return
	}
	p.release = func() { files(); repository() }
}
func (r *Runtime) openWorkspaceFile(sessionID protocol.SessionID, worktreeID protocol.WorktreeID, relative string) {
	session, ok := r.session(sessionID.String())
	if !ok || session.Worktree != worktreeID {
		r.enqueue(func(c *ui.Context) {
			r.reportKeys(c, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "workspace association changed"}, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
		})
		return
	}
	r.App.Sidebar.Selected = sessionID.String()
	r.workspaceFrame(nil)
	project := r.workspace.projects[worktreeID]
	if project == nil {
		return
	}
	project.panel.FileOpen(relative)
	r.App.PanelOpen = true
	r.App.Panel.Show(panel.Tab{Kind: panel.Files, ID: relative})
}
