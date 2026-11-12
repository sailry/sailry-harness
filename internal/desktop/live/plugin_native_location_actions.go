package live

import (
	"bytes"
	"context"
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginLocationSelection struct {
	Worktree   *protocol.WorktreeID `json:"worktree"`
	Project    json.RawMessage      `json:"project"`
	Fork       bool                 `json:"fork"`
	Projects   bool                 `json:"projects"`
	AddProject bool                 `json:"add_project"`
	Overview   bool                 `json:"overview"`
	Base       json.RawMessage      `json:"base"`
	Request    *protocol.RequestID  `json:"request"`
}

func (selection pluginLocationSelection) valid() bool {
	count := 0
	for _, selected := range []bool{
		selection.Worktree != nil, len(selection.Project) > 0,
		selection.Projects, selection.AddProject, selection.Overview,
		len(selection.Base) > 0 && string(selection.Base) != "null",
	} {
		if selected {
			count++
		}
	}
	return count == 1 && (len(selection.Base) == 0 || string(selection.Base) == "null" || !selection.Fork && selection.Request == nil)
}

func (location *pluginNativeLocation) selectLocation(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	cursor, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	if cursor == "" || len(arguments) < 2 {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid location selection")
	}
	var selection pluginLocationSelection
	decoder := json.NewDecoder(bytes.NewReader(arguments[1]))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&selection) != nil || !selection.valid() {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid location selection")
	}
	var confirmed *protocol.Worktree
	if selection.Request != nil {
		tree, err := location.confirm(ctx, cursor, selection.Worktree, *selection.Request)
		if err != nil {
			return nil, err
		}
		confirmed = &tree
	}
	return location.state.update(ctx, func(view *ui.Context) (json.RawMessage, error) {
		location.refresh()
		if err := location.check(cursor); err != nil {
			return nil, err
		}
		if confirmed != nil {
			location.remember(*confirmed)
		}
		if selection.Worktree != nil {
			if _, known := location.known[*selection.Worktree]; !known {
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "worktree is outside the captured project")
			}
		}
		var project *protocol.ProjectID
		if len(selection.Project) != 0 && string(selection.Project) != "null" {
			var id protocol.ProjectID
			if json.Unmarshal(selection.Project, &id) != nil {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid location selection")
			}
			for _, entry := range location.state.runtime.snapshot().Projects {
				if entry.ID == id {
					project = &id
					break
				}
			}
			if project == nil {
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "project is outside the captured Node")
			}
		}
		if location.composer != "" {
			if !location.control || selection.Overview {
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "conversation location control is unavailable")
			}
			return location.selectComposer(view, selection, project)
		}
		if selection.Projects || len(selection.Base) != 0 && string(selection.Base) != "null" ||
			len(selection.Project) != 0 || selection.AddProject || selection.Fork ||
			!location.state.runtime.controller.Connected() || location.state.runtime.localRuntime().selectedRuntime() != location.state.runtime {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "project location is unavailable")
		}
		if location.project == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "location requires a project")
		}
		root := location.state.runtime.localRuntime()
		for _, entry := range root.App.Data.Projects {
			if entry.Host == location.state.runtime.controller.Target().String() && entry.ID == location.project.String() {
				path := ""
				if selection.Worktree != nil {
					path = location.known[*selection.Worktree].Path
				}
				root.openProjectOverviewAt(entry, path)
				return pluginNativeJSON(true)
			}
		}
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "project context changed")
	})
}

func (location *pluginNativeLocation) selectComposer(view *ui.Context, selection pluginLocationSelection, project *protocol.ProjectID) (json.RawMessage, error) {
	state := location.state
	source, comp, config, err := state.composerSource()
	if err != nil {
		return nil, err
	}
	if !state.runtime.controller.Connected() || !state.runtime.canMoveWorkspace(source, comp) ||
		state.composerBusy(source.ID) || comp.Running || config.Assistant != nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation location is unavailable")
	}
	session, existing := state.runtime.session(source.ID)
	if len(selection.Base) != 0 && string(selection.Base) != "null" {
		worktree := composerPluginScope(state.runtime, source, comp).Worktree
		if existing || location.project == nil || worktree == nil || !location.repository(*worktree) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation location is unavailable")
		}
		var base sessionBase
		decoder := json.NewDecoder(bytes.NewReader(selection.Base))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&base) != nil || !base.valid() {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid session base")
		}
		state.runtime.sessionBases[comp] = &sessionBaseStart{base: base, project: *location.project, source: *worktree, worktree: comp.Worktree}
		return pluginNativeJSON(true)
	}
	if !selection.AddProject {
		delete(state.runtime.sessionBases, comp)
	}
	if len(selection.Project) > 0 {
		if existing || config.Resource != nil || len(comp.Attachments) != 0 {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "conversation project is fixed")
		}
		comp.Project, comp.Worktree = "", ""
		if project != nil {
			comp.Project = project.String()
			for _, tree := range state.runtime.snapshot().Worktrees {
				if tree.Project != nil && *tree.Project == *project && tree.Main {
					comp.Worktree = tree.Path
					break
				}
			}
		}
		return pluginNativeJSON(true)
	}
	if selection.AddProject {
		state.runtime.localRuntime().App.Sidebar.OpenEditor(nil)
		return pluginNativeJSON(true)
	}
	if selection.Projects {
		if existing || config.Resource != nil || len(comp.Attachments) != 0 {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "conversation project is fixed")
		}
		comp.OpenProjectPicker()
		return pluginNativeJSON(true)
	}
	if selection.Worktree == nil {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid location selection")
	}
	tree := location.known[*selection.Worktree]
	if tree.Project == nil || location.project == nil || *tree.Project != *location.project {
		return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "worktree is outside the captured project")
	}
	if !existing {
		if selection.Fork {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "fork requires a conversation")
		}
		comp.Worktree = tree.Path
		comp.References = nil
		return pluginNativeJSON(true)
	}
	if selection.Fork {
		state.runtime.executeScoped("plugin-location-fork/"+source.ID, protocol.Command{Kind: "fork_conversation_at", Data: protocol.CommandForkConversationAtPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Worktree: tree.ID,
		}}, func(current *ui.Context, output protocol.Output, failure error) {
			if failure != nil {
				state.runtime.packageReport(current, failure)
				return
			}
			if fork, valid := detail[protocol.Session](output.Data); valid {
				state.runtime.admittedSessions[fork.ID.String()] = fork
				state.runtime.project()
				state.runtime.OpenSession(fork.ID.String())
			}
		})
		return pluginNativeJSON(true)
	}
	if state.runtime.workspace == nil {
		state.runtime.initWorkspace()
	}
	captured, known := location.known[session.Worktree]
	if !known {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "captured worktree is unavailable")
	}
	current := state.runtime.workspaceFor(captured)
	if _, confirmed := location.confirmed[tree.ID]; confirmed {
		current.mutateNamed(workspaceAction, protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{
			Session: session.ID, Worktree: tree.ID, ExpectedRevision: session.Revision,
		}}, func(view *ui.Context, output protocol.Output) {
			if _, valid := detail[protocol.Session](output.Data); valid {
				comp.References = nil
				state.runtime.project()
				state.runtime.OpenSession(source.ID)
				kit.Toast(view, kit.Done, i18n.T(view, "workspace_completed"))
			}
		})
		return pluginNativeJSON(true)
	}
	state.runtime.selectConversationWorktree(source, comp, tree.Path)
	return pluginNativeJSON(true)
}

func (location *pluginNativeLocation) repository(worktree protocol.WorktreeID) bool {
	workspace := location.state.runtime.workspace
	if workspace == nil {
		return false
	}
	project := workspace.projects[worktree]
	return project != nil && project.statusKnown && project.status.Kind != protocol.RepositoryKindDirectory
}

func (location *pluginNativeLocation) hasDrafts(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	tree, err := pluginNativeArgument[protocol.WorktreeID](arguments, 0)
	if err != nil {
		return nil, err
	}
	var confirmed *protocol.Worktree
	if len(arguments) > 1 && string(arguments[1]) != "null" {
		request, err := pluginNativeArgument[protocol.RequestID](arguments, 1)
		if err != nil {
			return nil, err
		}
		value, err := location.confirm(ctx, "", &tree, request)
		if err != nil {
			return nil, err
		}
		confirmed = &value
	}
	return location.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		location.refresh()
		if err := location.check(""); err != nil {
			return nil, err
		}
		if confirmed != nil {
			location.remember(*confirmed)
		}
		if _, known := location.known[tree]; !known {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "worktree is outside the captured project")
		}
		return pluginNativeJSON(location.worktreeDrafts(tree))
	})
}

func (location *pluginNativeLocation) worktreeDrafts(tree protocol.WorktreeID) bool {
	if workspace := location.state.runtime.workspace; workspace != nil {
		if project := workspace.projects[tree]; project != nil {
			for _, name := range project.panel.Files.Open() {
				if _, dirty := project.panel.FileDraft(name); dirty {
					return true
				}
			}
		}
	}
	return false
}
