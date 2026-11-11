package live

import (
	"reflect"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceSettingsState struct {
	worktree        protocol.WorktreeID
	hooks           protocol.HooksSettings
	sandboxed       bool
	sandboxRevision uint64
	loading         bool
	hookModels      map[protocol.WorktreeID]*settings.Stored[settings.HookSettings]
	sandboxModel    settings.Stored[bool]
	sandboxKnown    bool
}

func (r *Runtime) initWorkspaceSettings() {
	state := &r.workspaceSettings
	if state.hookModels != nil {
		return
	}
	state.hookModels = map[protocol.WorktreeID]*settings.Stored[settings.HookSettings]{}
	if owner := r.localRuntime().workspaceSettingsOwner; owner == nil || owner == r {
		state.sandboxModel = r.App.Settings.Sandbox
		if state.worktree != (protocol.WorktreeID{}) {
			value := r.App.Settings.Hooks
			state.hookModels[state.worktree] = &value
		}
	}
}
func (r *Runtime) workspaceHooksModel(id protocol.WorktreeID) *settings.Stored[settings.HookSettings] {
	r.initWorkspaceSettings()
	if model := r.workspaceSettings.hookModels[id]; model != nil {
		return model
	}
	model := &settings.Stored[settings.HookSettings]{}
	r.workspaceSettings.hookModels[id] = model
	return model
}
func (r *Runtime) captureWorkspaceSettings() {
	if r.localRuntime().workspaceSettingsOwner != r {
		return
	}
	r.initWorkspaceSettings()
	r.workspaceSettings.sandboxModel = r.App.Settings.Sandbox
	if id := r.workspaceSettings.worktree; id != (protocol.WorktreeID{}) {
		*r.workspaceHooksModel(id) = r.App.Settings.Hooks
	}
}
func (r *Runtime) publishWorkspaceSettings() {
	if r.localRuntime().workspaceSettingsOwner != r {
		return
	}
	r.App.Settings.Sandbox = r.workspaceSettings.sandboxModel
	r.App.Settings.Hooks = settings.Stored[settings.HookSettings]{}
	if id := r.workspaceSettings.worktree; id != (protocol.WorktreeID{}) {
		r.App.Settings.Hooks = *r.workspaceHooksModel(id)
	}
}
func (r *Runtime) bindWorkspaceSettings() {
	root := r.localRuntime()
	r.initWorkspaceSettings()
	root.workspaceSettingsOwner = r
	root.App.Settings.SaveSandbox = r.saveSandbox
	root.App.Settings.SaveHooks = r.saveWorkspaceHooks
	root.App.Settings.HooksContext = r.hookContext
	root.App.Settings.HooksSandboxed = r.hooksSandboxed
	r.publishWorkspaceSettings()
}

// workspaceSettingsFrame keeps the sandbox and the hooks on the Node
// Settings edits (settings_target.go), holding each Node's drafts while
// another shows. A host without a connection has neither: the sandbox
// switch waits and the hooks page asks for a project.
func (r *Runtime) workspaceSettingsFrame() {
	root := r.localRuntime()
	owner := root.settingsOwner()
	previous := root.workspaceSettingsOwner
	if previous != nil {
		previous.captureWorkspaceSettings()
		if root.App.Settings.HooksEditing() {
			owner = previous
		}
	}
	if owner == nil {
		if previous != nil {
			m := root.App.Settings
			root.workspaceSettingsOwner = nil
			m.SaveSandbox, m.SaveHooks, m.HooksSandboxed = nil, nil, nil
			m.HooksContext = func() (string, bool) { return "", false }
			m.Sandbox, m.Hooks = settings.Stored[bool]{}, settings.Stored[settings.HookSettings]{}
		}
		return
	}
	if previous != owner {
		owner.bindWorkspaceSettings()
	}
	owner.refreshWorkspaceSettings()
}
func (r *Runtime) refreshWorkspaceSettings() {
	if r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	r.initWorkspaceSettings()
	state := &r.workspaceSettings
	if (!state.sandboxKnown || r.snapshot().SandboxSettingsRevision != state.sandboxRevision) && !state.sandboxModel.Pending {
		r.readSandbox()
	}
	// The hooks are those of the conversation shown, when it runs on this
	// Node; a conversation on another host has its hooks on that host.
	var id protocol.WorktreeID
	if r.localRuntime().selectedRuntime() == r {
		session, ok := r.session(r.App.Sidebar.Selected)
		if !ok {
			session, ok = r.draftWorkspaceSession(r.App.Sidebar.Selected)
		}
		if ok && session.Project != nil {
			id = session.Worktree
		}
	}
	if id != state.worktree && !r.App.Settings.HooksEditing() {
		state.worktree = id
		state.hooks = protocol.DefaultHooksSettings()
		state.sandboxed = false
		state.loading = false
		if id != (protocol.WorktreeID{}) {
			r.readWorkspaceHooks(id)
		}
	}
	r.publishWorkspaceSettings()
}
func (r *Runtime) hookContext() (string, bool) {
	id := r.workspaceSettings.worktree
	if id == (protocol.WorktreeID{}) {
		return "", false
	}
	var tree protocol.Worktree
	found := false
	for _, candidate := range r.snapshot().Worktrees {
		if candidate.ID == id {
			tree, found = candidate, true
			break
		}
	}
	if !found && r.workspace != nil {
		tree, found = r.workspace.admitted[id]
	}
	if !found || tree.Project == nil {
		return "", false
	}
	label := r.controller.Target().String() + " · " + tree.Path + " · " + tree.ID.String()
	if r.workspace != nil {
		if project := r.workspace.projects[id]; project != nil && project.status.Branch != nil {
			label += " · " + *project.status.Branch
		}
	}
	return label, true
}
func (r *Runtime) hooksSandboxed() bool {
	return r.workspaceSettings.sandboxed
}
func hookDraft(value protocol.HooksSettings) settings.HookSettings {
	draft := settings.HookSettings{Enabled: value.Enabled, Scripts: []viewmodel.Hook{}}
	add := func(id string, phase viewmodel.HookPhase, script *protocol.HooksScript) {
		if script != nil {
			draft.Scripts = append(draft.Scripts, viewmodel.Hook{ID: id, Name: script.Name, Command: script.Command, Phase: phase, TimeoutMS: int(script.TimeoutMs), Enabled: script.Enabled})
		}
	}
	add("before_tool", viewmodel.BeforeTool, value.BeforeTool)
	add("after_tool", viewmodel.AfterTool, value.AfterTool)
	add("turn_end", viewmodel.TurnEnd, value.TurnEnd)
	for _, entry := range value.Additional {
		phase := viewmodel.BeforeTool
		switch entry.Phase {
		case protocol.HooksPhaseAfterTool:
			phase = viewmodel.AfterTool
		case protocol.HooksPhaseTurnEnd:
			phase = viewmodel.TurnEnd
		}
		add(entry.ID, phase, &entry.Script)
	}
	return draft
}
func (r *Runtime) readWorkspaceHooks(id protocol.WorktreeID) {
	model := r.workspaceHooksModel(id)
	if model.Pending {
		return
	}
	r.workspaceSettings.loading = true
	model.Pending = true
	r.publishWorkspaceSettings()
	r.execute(protocol.Command{Kind: "read_hooks", Data: protocol.CommandReadHooksPayload{Worktree: id}}, func(c *ui.Context, out protocol.Output, err error) {
		model.Pending = false
		if id == r.workspaceSettings.worktree {
			r.workspaceSettings.loading = false
		}
		defer r.publishWorkspaceSettings()
		if err != nil {
			r.reportKeys(c, err, "hooks_failed", "hooks_conflict", "hooks_unknown")
			return
		}
		value, ok := detail[protocol.HooksState](out.Data)
		if !ok {
			return
		}
		draft := hookDraft(value.Settings)
		if reflect.DeepEqual(model.Draft, model.Saved) {
			model.Draft = draft
		}
		model.Saved, model.Revision = draft, value.Settings.Revision
		if id == r.workspaceSettings.worktree {
			r.workspaceSettings.hooks = value.Settings
			r.workspaceSettings.sandboxed = value.Sandboxed
		}
	})
}
func (r *Runtime) saveWorkspaceHooks() {
	if r.ctx != nil && r.ctx.Err() != nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the hooks execution host is unavailable"))
		return
	}
	r.captureWorkspaceSettings()
	id := r.workspaceSettings.worktree
	if id == (protocol.WorktreeID{}) {
		return
	}
	m := r.workspaceHooksModel(id)
	if m.Pending {
		return
	}
	m.Pending = true
	r.publishWorkspaceSettings()
	value := protocol.DefaultHooksSettings()
	value.Revision = m.Revision
	value.Enabled = m.Draft.Enabled
	for _, entry := range m.Draft.Scripts {
		phase := protocol.HooksPhaseBeforeTool
		switch entry.Phase {
		case viewmodel.AfterTool:
			phase = protocol.HooksPhaseAfterTool
		case viewmodel.TurnEnd:
			phase = protocol.HooksPhaseTurnEnd
		}
		script := protocol.HooksScript{Name: entry.Name, Enabled: entry.Enabled, Command: entry.Command, TimeoutMs: uint64(entry.TimeoutMS)}
		switch entry.ID {
		case "before_tool":
			value.BeforeTool = &script
		case "after_tool":
			value.AfterTool = &script
		case "turn_end":
			value.TurnEnd = &script
		default:
			value.Additional = append(value.Additional, protocol.HooksEntry{ID: entry.ID, Phase: phase, Script: script})
		}
	}
	r.saveSetting(protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Worktree: id, Settings: value, ExpectedRevision: m.Revision}}, settings.Messages{Done: "hooks_saved", Failed: "hooks_failed", Conflict: "hooks_conflict", Uncertain: "hooks_unknown"}, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishWorkspaceSettings()
		if !ok {
			return
		}
		state, valid := detail[protocol.HooksState](out.Data)
		if !valid {
			return
		}
		m.Saved, m.Draft, m.Revision = hookDraft(state.Settings), hookDraft(state.Settings), state.Settings.Revision
		if id == r.workspaceSettings.worktree {
			r.workspaceSettings.hooks = state.Settings
			r.workspaceSettings.sandboxed = state.Sandboxed
		}
	})
}
func (r *Runtime) readSandbox() {
	r.initWorkspaceSettings()
	m := &r.workspaceSettings.sandboxModel
	if m.Pending {
		return
	}
	m.Pending = true
	r.workspaceSettings.sandboxRevision = r.snapshot().SandboxSettingsRevision
	r.workspaceSettings.sandboxKnown = true
	r.publishWorkspaceSettings()
	r.execute(protocol.Command{Kind: "read_sandbox_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		m.Pending = false
		defer r.publishWorkspaceSettings()
		if err != nil {
			r.reportKeys(c, err, "sandbox_settings_failed", "sandbox_settings_conflict", "sandbox_settings_unknown")
			return
		}
		value, ok := detail[protocol.SandboxSettings](out.Data)
		if !ok {
			return
		}
		if m.Draft == m.Saved {
			m.Draft = value.Enabled
		}
		m.Saved, m.Revision = value.Enabled, value.Revision
		r.workspaceSettings.sandboxRevision = value.Revision
	})
}
func (r *Runtime) saveSandbox() {
	if r.ctx != nil && r.ctx.Err() != nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the sandbox execution host is unavailable"))
		return
	}
	r.captureWorkspaceSettings()
	m := &r.workspaceSettings.sandboxModel
	if m.Pending {
		return
	}
	m.Pending = true
	r.publishWorkspaceSettings()
	r.saveSetting(protocol.Command{Kind: "save_sandbox_settings", Data: protocol.SandboxSettings{Revision: m.Revision, Enabled: m.Draft}}, settings.Messages{Done: "sandbox_settings_saved", Failed: "sandbox_settings_failed", Conflict: "sandbox_settings_conflict", Uncertain: "sandbox_settings_unknown"}, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishWorkspaceSettings()
		if !ok {
			return
		}
		value, valid := detail[protocol.SandboxSettings](out.Data)
		if !valid {
			return
		}
		m.Saved, m.Revision = value.Enabled, value.Revision
		r.workspaceSettings.sandboxRevision = value.Revision
	})
}
