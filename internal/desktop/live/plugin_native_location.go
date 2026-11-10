package live

import (
	"bytes"
	"context"
	"encoding/json"
	"slices"
	"strconv"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeLocation struct {
	state     *pluginNativeState
	mu        sync.Mutex
	source    json.RawMessage
	value     json.RawMessage
	cursor    uint64
	failure   error
	changed   chan struct{}
	known     map[protocol.WorktreeID]protocol.Worktree
	confirmed map[protocol.WorktreeID]protocol.Worktree
	project   *protocol.ProjectID
	captured  bool
	composer  string
	read      bool
	write     bool
	control   bool
}

func (state *pluginNativeState) locationOperations() {
	info, available := state.packageInfo()
	location := &pluginNativeLocation{
		state: state, changed: make(chan struct{}), known: map[protocol.WorktreeID]protocol.Worktree{},
		confirmed: map[protocol.WorktreeID]protocol.Worktree{},
	}
	if available && info.Extension != nil {
		location.read = slices.Contains(info.Extension.Actions, protocol.PluginActionReadWorktrees)
		location.write = slices.Contains(info.Extension.Actions, protocol.PluginActionWriteWorktrees)
		location.control = slices.Contains(info.Extension.Actions, protocol.PluginActionControlConversation)
	}
	if state.resource == nil && state.composer != nil {
		location.composer = state.composer.source
	}
	state.native.Operations["sailry/ui.readLocation"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		location.refresh()
		if err := location.check(""); err != nil {
			return nil, err
		}
		location.mu.Lock()
		defer location.mu.Unlock()
		return append(json.RawMessage{}, location.value...), nil
	}
	state.native.Operations["sailry/ui.nextLocationChange"] = location.next
	state.native.Operations["sailry/ui.selectLocation"] = location.selectLocation
	state.native.Operations["sailry/ui.worktreeHasDrafts"] = location.hasDrafts
	state.native.Operations["sailry/ui.releaseWorktree"] = location.release
	frame := state.native.Frame
	state.native.Frame = func(view *ui.Context) {
		if frame != nil {
			frame(view)
		}
		location.refresh()
	}
	location.refresh()
}

func (location *pluginNativeLocation) current() (map[string]any, error) {
	state := location.state
	if err := state.native.Context.Err(); err != nil {
		return nil, err
	}
	snapshot := state.runtime.controller.Snapshot()
	value := map[string]any{
		"surface": "project", "node": state.runtime.controller.Target().String(),
		"project": nil, "worktree": state.scope.Worktree, "session": nil, "revision": nil,
		"main": true, "project_name": "", "branch": "", "can_move": state.runtime.controller.Connected(),
		"can_retarget": false, "projects": []map[string]any{}, "git": false, "git_status": "unknown",
		"connected": state.runtime.controller.Connected(),
	}
	var source *viewmodel.Conversation
	var comp *composer.State
	var config protocol.SessionConfig
	worktree := state.scope.Worktree
	if location.composer != "" {
		var err error
		source, comp, config, err = state.composerSource()
		if err != nil {
			return nil, err
		}
		if state.runtime.conversationRuntime(source, comp) != state.runtime {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation context changed")
		}
		captured := composerPluginScope(state.runtime, source, comp)
		worktree = captured.Worktree
		value["surface"], value["session"], value["base"] = "composer", captured.Session, nil
		if start := state.runtime.pendingSessionBase(comp); start != nil && captured.Session == nil {
			value["base"] = start.base
		}
		busy := state.composerBusy(source.ID)
		retarget := captured.Session == nil && config.Resource == nil && config.Assistant == nil && !busy && !comp.Running
		if state.runtime.attachments != nil {
			for _, attachment := range comp.Attachments {
				record := state.runtime.attachments.items[attachment]
				if record != nil && (record.running || record.removing || record.pending != nil) {
					retarget = false
				}
			}
		}
		value["can_retarget"] = retarget
		value["can_move"] = state.runtime.controller.Connected() && config.Assistant == nil && !busy && !comp.Running &&
			state.runtime.canMoveWorkspace(source, comp)
		if session, exists := state.runtime.session(source.ID); exists {
			value["revision"] = strconv.FormatUint(session.Revision, 10)
		}
		if retarget {
			projects := []map[string]any{}
			for _, project := range snapshot.Projects {
				projects = append(projects, map[string]any{"id": project.ID, "name": project.Name})
			}
			value["projects"] = projects
		}
	}
	value["worktree"] = worktree
	var project *protocol.ProjectID
	if worktree != nil {
		for _, tree := range snapshot.Worktrees {
			if tree.ID != *worktree {
				continue
			}
			project = tree.Project
			value["main"] = tree.Main
			break
		}
		if project == nil {
			if tree, known := location.known[*worktree]; known {
				project = tree.Project
				value["main"] = tree.Main
			}
		}
	}
	if project == nil && comp != nil && comp.Project != "" {
		if id, err := protocol.ParseProjectID(comp.Project); err == nil {
			project = &id
		}
	}
	if !location.captured {
		if project != nil {
			location.project = new(*project)
		}
		location.captured = true
	}
	value["project"] = project
	for _, entry := range snapshot.Projects {
		if project != nil && entry.ID == *project {
			value["project_name"] = entry.Name
			break
		}
	}
	for _, tree := range snapshot.Worktrees {
		if worktree != nil && tree.ID == *worktree ||
			location.project != nil && tree.Project != nil && *tree.Project == *location.project {
			location.known[tree.ID] = tree
		}
	}
	if worktree != nil && state.runtime.workspace != nil {
		if project := state.runtime.workspace.projects[*worktree]; project != nil && project.statusKnown {
			branch := workspaceString(project.status.Branch)
			if branch == "" {
				head := workspaceString(project.status.Head)
				branch = head[:min(7, len(head))]
			}
			value["branch"] = branch
			if source != nil {
				value["git"] = project.status.Kind != protocol.RepositoryKindDirectory
				value["git_status"] = "repository"
				if project.status.Kind == protocol.RepositoryKindDirectory {
					value["git_status"] = "plain_directory"
				}
			}
		}
	}
	return value, nil
}

func (location *pluginNativeLocation) refresh() {
	value, failure := location.current()
	body, err := json.Marshal(value)
	if failure == nil {
		failure = err
	}
	location.mu.Lock()
	defer location.mu.Unlock()
	equalError := location.failure == nil && failure == nil ||
		location.failure != nil && failure != nil && location.failure.Error() == failure.Error()
	if bytes.Equal(location.source, body) && equalError {
		return
	}
	location.cursor++
	location.source, location.failure = body, failure
	if failure == nil {
		value["cursor"] = strconv.FormatUint(location.cursor, 10)
		location.value, location.failure = json.Marshal(value)
	}
	close(location.changed)
	location.changed = make(chan struct{})
}

func (location *pluginNativeLocation) check(cursor string) error {
	state := location.state
	if err := state.native.Context.Err(); err != nil {
		return err
	}
	if !location.read {
		return pluginNativeFault(protocol.ErrorCodePermissionDenied, "location read is unavailable")
	}
	location.mu.Lock()
	defer location.mu.Unlock()
	if location.failure != nil {
		return location.failure
	}
	var value struct {
		Node     string               `json:"node"`
		Worktree *protocol.WorktreeID `json:"worktree"`
		Session  *protocol.SessionID  `json:"session"`
	}
	if json.Unmarshal(location.value, &value) != nil || value.Node != state.runtime.controller.Target().String() ||
		pluginWorktreeKey(value.Worktree) != pluginWorktreeKey(state.scope.Worktree) ||
		pluginSessionKey(value.Session) != pluginSessionKey(state.scope.Session) {
		return pluginNativeFault(protocol.ErrorCodePermissionDenied, "location is outside the captured view")
	}
	if cursor != "" && cursor != strconv.FormatUint(location.cursor, 10) {
		return pluginNativeFault(protocol.ErrorCodeRevisionConflict, "location changed")
	}
	return nil
}

func (location *pluginNativeLocation) next(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if !location.read {
		return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "location read is unavailable")
	}
	seen, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	for {
		location.mu.Lock()
		body, failure := append(json.RawMessage{}, location.value...), location.failure
		cursor, changed := location.cursor, location.changed
		location.mu.Unlock()
		if failure != nil {
			return nil, failure
		}
		if strconv.FormatUint(cursor, 10) != seen {
			return body, nil
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-location.state.native.Context.Done():
			return nil, location.state.native.Context.Err()
		case <-changed:
		}
	}
}
