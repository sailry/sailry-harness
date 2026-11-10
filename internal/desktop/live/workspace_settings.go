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
}

func (r *Runtime) workspaceSettingsFrame() {
	state := &r.workspaceSettings
	if r.controller.Snapshot().SandboxSettingsRevision != state.sandboxRevision && !r.App.Settings.Sandbox.Pending {
		r.readSandbox()
	}
	session, ok := r.session(r.App.Sidebar.Selected)
	if !ok {
		session, ok = r.draftWorkspaceSession(r.App.Sidebar.Selected)
	}
	var id protocol.WorktreeID
	if ok && session.Project != nil {
		id = session.Worktree
	}
	if id != state.worktree {
		state.worktree = id
		state.hooks = protocol.DefaultHooksSettings()
		state.sandboxed = false
		state.loading = false
		r.App.Settings.Hooks = settings.Stored[settings.HookSettings]{}
		if id != (protocol.WorktreeID{}) {
			r.readWorkspaceHooks(id)
		}
	}
}
func (r *Runtime) hookContext() (string, bool) {
	id := r.workspaceSettings.worktree
	if id == (protocol.WorktreeID{}) {
		return "", false
	}
	var tree protocol.Worktree
	found := false
	for _, candidate := range r.controller.Snapshot().Worktrees {
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
	r.workspaceSettings.loading = true
	r.App.Settings.Hooks.Pending = true
	r.execute(protocol.Command{Kind: "read_hooks", Data: protocol.CommandReadHooksPayload{Worktree: id}}, func(c *ui.Context, out protocol.Output, err error) {
		if id != r.workspaceSettings.worktree {
			return
		}
		r.workspaceSettings.loading = false
		m := &r.App.Settings.Hooks
		m.Pending = false
		if err != nil {
			r.reportKeys(c, err, "hooks_failed", "hooks_conflict", "hooks_unknown")
			return
		}
		value, ok := detail[protocol.HooksState](out.Data)
		if !ok {
			return
		}
		draft := hookDraft(value.Settings)
		if reflect.DeepEqual(m.Draft, m.Saved) {
			m.Draft = draft
		}
		m.Saved, m.Revision = draft, value.Settings.Revision
		r.workspaceSettings.hooks = value.Settings
		r.workspaceSettings.sandboxed = value.Sandboxed
	})
}
func (r *Runtime) saveWorkspaceHooks() {
	id := r.workspaceSettings.worktree
	if id == (protocol.WorktreeID{}) {
		return
	}
	m := &r.App.Settings.Hooks
	if m.Pending {
		return
	}
	m.Pending = true
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
		if id != r.workspaceSettings.worktree {
			return
		}
		m.Pending = false
		if !ok {
			return
		}
		state, valid := detail[protocol.HooksState](out.Data)
		if !valid {
			return
		}
		m.Saved, m.Draft, m.Revision = hookDraft(state.Settings), hookDraft(state.Settings), state.Settings.Revision
		r.workspaceSettings.hooks = state.Settings
		r.workspaceSettings.sandboxed = state.Sandboxed
	})
}
func (r *Runtime) readSandbox() {
	m := &r.App.Settings.Sandbox
	if m.Pending {
		return
	}
	m.Pending = true
	r.workspaceSettings.sandboxRevision = r.controller.Snapshot().SandboxSettingsRevision
	r.execute(protocol.Command{Kind: "read_sandbox_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		m.Pending = false
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
	m := &r.App.Settings.Sandbox
	if m.Pending {
		return
	}
	m.Pending = true
	r.saveSetting(protocol.Command{Kind: "save_sandbox_settings", Data: protocol.SandboxSettings{Revision: m.Revision, Enabled: m.Draft}}, settings.Messages{Done: "sandbox_settings_saved", Failed: "sandbox_settings_failed", Conflict: "sandbox_settings_conflict", Uncertain: "sandbox_settings_unknown"}, func(out protocol.Output, ok bool) {
		m.Pending = false
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
