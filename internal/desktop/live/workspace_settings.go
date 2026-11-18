package live

import (
	"reflect"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceSettingsState struct {
	worktree        protocol.WorktreeID
	contextWorktree protocol.WorktreeID
	chosenWorktree  protocol.WorktreeID
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
	if r.parent == nil {
		state.sandboxModel = r.App.Settings.Sandbox
	}
	if owner := r.localRuntime().workspaceSettingsOwner; owner == nil || owner == r {
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
	r.initWorkspaceSettings()
	if r.localRuntime().workspaceSettingsOwner != r {
		return
	}
	if id := r.workspaceSettings.worktree; id != (protocol.WorktreeID{}) {
		*r.workspaceHooksModel(id) = r.App.Settings.Hooks
	}
}
func (r *Runtime) publishWorkspaceSettings() {
	if r.localRuntime().workspaceSettingsOwner != r {
		return
	}
	r.App.Settings.Hooks = settings.Stored[settings.HookSettings]{}
	if id := r.workspaceSettings.worktree; id != (protocol.WorktreeID{}) {
		r.App.Settings.Hooks = *r.workspaceHooksModel(id)
	}
}
func (r *Runtime) bindWorkspaceSettings() {
	root := r.localRuntime()
	r.initWorkspaceSettings()
	root.workspaceSettingsOwner = r
	root.App.Settings.SaveHooks = r.saveWorkspaceHooks
	root.App.Settings.HooksContext = r.hookContext
	root.App.Settings.HooksSandboxed = r.hooksSandboxed
	root.App.Settings.SelectHookWorktree = r.selectHookWorktree
	r.publishWorkspaceSettings()
}

// workspaceSettingsFrame follows the settings host for sandbox configuration
// and the worktree for hooks. Hook editors retain their captured context.
func (r *Runtime) workspaceSettingsFrame() {
	root := r.localRuntime()
	root.sandboxFrame()
	owner := root.selectedRuntime()
	previous := root.workspaceSettingsOwner
	if previous != nil {
		previous.captureWorkspaceSettings()
		if root.App.Settings.HooksEditing() {
			owner = previous
		}
	}
	if owner == nil {
		root.workspaceSettingsOwner = nil
		m := root.App.Settings
		m.SaveHooks, m.HooksSandboxed = nil, nil
		m.HooksContext = func() (settings.HookTarget, bool) { return settings.HookTarget{}, false }
		m.SelectHookWorktree = nil
		m.Hooks = settings.Stored[settings.HookSettings]{}
	} else {
		if previous != owner {
			owner.bindWorkspaceSettings()
		}
		owner.refreshWorkspaceSettings()
	}
}

func (r *Runtime) sandboxFrame() {
	owner := r.shownSettings()
	m := r.App.Settings
	if owner.machineSettingsStatus() != "" {
		m.SaveSandbox = nil
		m.Sandbox = settings.Stored[bool]{Pending: true}
		return
	}
	owner.initWorkspaceSettings()
	state := &owner.workspaceSettings
	m.SaveSandbox = func() {
		if owner.showsSettings() && owner.machineSettingsStatus() == "" {
			state.sandboxModel.Draft = m.Sandbox.Draft
			owner.saveSandbox()
		}
	}
	if (!state.sandboxKnown || owner.snapshot().SandboxSettingsRevision > state.sandboxRevision) && !state.sandboxModel.Pending {
		owner.readSandbox()
	}
	owner.publishSandbox()
}

func (r *Runtime) refreshWorkspaceSettings() {
	if r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	r.initWorkspaceSettings()
	state := &r.workspaceSettings
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
	if r.App.Settings.HooksEditing() {
		r.publishWorkspaceSettings()
		return
	}
	if id != state.contextWorktree {
		state.contextWorktree, state.chosenWorktree = id, protocol.WorktreeID{}
	}
	if state.chosenWorktree != (protocol.WorktreeID{}) {
		if _, found := r.hookWorktree(state.chosenWorktree); found {
			id = state.chosenWorktree
		} else {
			state.chosenWorktree = protocol.WorktreeID{}
		}
	}
	if id != state.worktree {
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
func (r *Runtime) hookWorktree(id protocol.WorktreeID) (protocol.Worktree, bool) {
	if id == (protocol.WorktreeID{}) {
		return protocol.Worktree{}, false
	}
	for _, tree := range r.snapshot().Worktrees {
		if tree.ID == id {
			return tree, tree.Project != nil
		}
	}
	if r.workspace != nil {
		tree, found := r.workspace.admitted[id]
		return tree, found && tree.Project != nil
	}
	return protocol.Worktree{}, false
}

func (r *Runtime) hookContext() (settings.HookTarget, bool) {
	tree, ok := r.hookWorktree(r.workspaceSettings.worktree)
	if !ok {
		return settings.HookTarget{}, false
	}
	target := settings.HookTarget{
		Host: r.packageHostLabel(), Project: tree.Project.String(), Path: tree.Path, Worktree: tree.ID.String(),
		Worktrees: []settings.HookWorktree{{ID: tree.ID.String(), Path: tree.Path}},
	}
	snapshot := r.snapshot()
	for _, project := range snapshot.Projects {
		if project.ID == *tree.Project {
			target.Project = project.Name
			break
		}
	}
	appendTree := func(candidate protocol.Worktree) {
		if candidate.ID != tree.ID && candidate.Project != nil && *candidate.Project == *tree.Project &&
			!slices.ContainsFunc(target.Worktrees, func(x settings.HookWorktree) bool { return x.ID == candidate.ID.String() }) {
			target.Worktrees = append(target.Worktrees, settings.HookWorktree{ID: candidate.ID.String(), Path: candidate.Path})
		}
	}
	for _, candidate := range snapshot.Worktrees {
		appendTree(candidate)
	}
	if r.workspace != nil {
		for _, candidate := range r.workspace.admitted {
			appendTree(candidate)
		}
	}
	slices.SortFunc(target.Worktrees, func(a, b settings.HookWorktree) int { return strings.Compare(a.Path, b.Path) })
	return target, true
}

func (r *Runtime) selectHookWorktree(value string) {
	if r.localRuntime().workspaceSettingsOwner != r || r.App.Settings.HooksEditing() || r.App.Settings.Hooks.Pending {
		return
	}
	id, err := protocol.ParseWorktreeID(value)
	if err != nil {
		return
	}
	base, baseValid := r.hookWorktree(r.workspaceSettings.contextWorktree)
	chosen, chosenValid := r.hookWorktree(id)
	if baseValid && chosenValid && *base.Project == *chosen.Project {
		r.workspaceSettings.chosenWorktree = id
		r.refreshWorkspaceSettings()
	}
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
	// Retry saves the draft again while the page still shows this project.
	again := func() {
		if r.workspaceSettings.worktree == id {
			r.saveWorkspaceHooks()
		}
	}
	r.saveSettingRetry(protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Worktree: id, Settings: value, ExpectedRevision: m.Revision}}, settings.Messages{Done: "hooks_saved", Failed: "hooks_failed", Conflict: "hooks_conflict", Uncertain: "hooks_unknown"}, nil, again, func(out protocol.Output, ok bool) {
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
	r.publishSandbox()
	r.execute(protocol.Command{Kind: "read_sandbox_settings"}, func(c *ui.Context, out protocol.Output, err error) {
		m.Pending = false
		defer r.publishSandbox()
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
	m := &r.workspaceSettings.sandboxModel
	if m.Pending {
		return
	}
	m.Pending = true
	r.publishSandbox()
	r.saveSettingRetry(protocol.Command{Kind: "save_sandbox_settings", Data: protocol.SandboxSettings{Revision: m.Revision, Enabled: m.Draft}}, settings.Messages{Done: "sandbox_settings_saved", Failed: "sandbox_settings_failed", Conflict: "sandbox_settings_conflict", Uncertain: "sandbox_settings_unknown"}, nil, r.saveSandbox, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishSandbox()
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

func (r *Runtime) publishSandbox() {
	if r.showsSettings() {
		r.App.Settings.Sandbox = r.workspaceSettings.sandboxModel
	}
}
