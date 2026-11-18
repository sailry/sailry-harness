package live

import (
	"cmp"
	"reflect"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceSettingsState struct {
	worktree        protocol.WorktreeID
	hookTarget      settings.HookTarget
	hookGeneration  uint64 // root's current Hooks binding
	hookReload      bool
	sandboxed       bool
	sandboxRevision uint64
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
func (r *Runtime) publishWorkspaceSettings() {
	root := r.localRuntime()
	if root.workspaceSettingsOwner != r || !root.hooksSettingsOpen() {
		return
	}
	id := r.workspaceSettings.worktree
	model := r.workspaceHooksModel(id)
	m := root.App.Settings
	m.Hooks, m.HooksContext, m.HooksSandboxed = *model, r.hookContext, r.hooksSandboxed
	generation, revision := root.workspaceSettings.hookGeneration, model.Revision
	current := func() bool {
		return root.workspaceSettingsOwner == r && root.workspaceSettings.hookGeneration == generation &&
			root.hooksSettingsOpen() && r.workspaceSettings.worktree == id && model.Revision == revision
	}
	m.SaveHooks = func(draft settings.HookSettings) {
		if !current() {
			return
		}
		if _, available := r.hookContext(); !available {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the hooks execution host is unavailable"))
			return
		}
		r.saveWorkspaceHooks(id, draft, current)
	}
}
func (r *Runtime) bindWorkspaceSettings(id protocol.WorktreeID, target settings.HookTarget) {
	root := r.localRuntime()
	r.initWorkspaceSettings()
	state := &r.workspaceSettings
	m := root.App.Settings
	entered := root.workspaceSettingsOwner != r || id != state.worktree || m.SaveHooks == nil
	if entered {
		root.workspaceSettings.hookGeneration++
	}
	root.workspaceSettingsOwner = r
	state.worktree, state.hookTarget = id, target
	if entered {
		state.sandboxed, state.hookReload = false, true
	}
	if state.hookReload && !r.workspaceHooksModel(id).Pending {
		state.hookReload = false
		r.readWorkspaceHooks(id)
	}
	r.publishWorkspaceSettings()
}

// workspaceSettingsFrame follows the settings host for sandbox configuration
// and the worktree for hooks. Hook editors retain their captured context.
func (r *Runtime) workspaceSettingsFrame() {
	root := r.localRuntime()
	root.sandboxFrame()
	m := root.App.Settings
	if !root.hooksSettingsOpen() {
		if m.HooksContext != nil {
			root.workspaceSettings.hookGeneration++
			m.HooksContext, m.SaveHooks, m.HooksSandboxed = nil, nil, nil
		}
		return
	}
	if m.HookTargets == nil {
		m.HookTargets, m.SelectHookTarget = root.workspaceHookTargets, root.selectHookTarget
	}
	root.refreshWorkspaceHooks()
}

func (r *Runtime) hooksSettingsOpen() bool {
	return r.App.Page == shell.SettingsPage && r.App.Settings.Section == settings.Hooks
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

func (r *Runtime) refreshWorkspaceHooks() {
	m := r.App.Settings
	previous := r.workspaceSettingsOwner
	target := m.HookSelection
	var owner *Runtime
	var id protocol.WorktreeID
	switch {
	case previous != nil && m.HooksEditing():
		owner, id, target = previous, previous.workspaceSettings.worktree, previous.workspaceSettings.hookTarget
	case target.Node != "":
		owner = r.runtimeFor(target.Node)
		id, _ = protocol.ParseWorktreeID(target.Worktree)
	default:
		owner = r.selectedRuntime()
		if owner != nil {
			id = owner.currentHookWorktree()
		}
	}
	if owner != nil && id != (protocol.WorktreeID{}) {
		if tree, ok := owner.hookWorktree(id); ok {
			projects, _ := owner.hookResources()
			name := ""
			if i := slices.IndexFunc(projects, func(project protocol.Project) bool { return project.ID == *tree.Project }); i >= 0 {
				name = projects[i].Name
			}
			target = owner.hookTarget(tree, name)
			if owner.hookOwnerAvailable() {
				owner.bindWorkspaceSettings(id, target)
				return
			}
		}
	}
	if !m.HooksEditing() && previous != nil {
		r.workspaceSettingsOwner = nil
		r.workspaceSettings.hookGeneration++
	}
	m.SaveHooks, m.HooksSandboxed = nil, nil
	m.HooksContext = func() (settings.HookTarget, bool) { return target, false }
	if !m.HooksEditing() {
		m.Hooks = settings.Stored[settings.HookSettings]{}
	}
}

func (r *Runtime) currentHookWorktree() protocol.WorktreeID {
	session, ok := r.session(r.App.Sidebar.Selected)
	if !ok && r.workspace != nil {
		session, ok = r.draftWorkspaceSession(r.App.Sidebar.Selected)
	}
	if ok && session.Project != nil {
		return session.Worktree
	}
	return protocol.WorktreeID{}
}

func (r *Runtime) hookOwnerAvailable() bool {
	root, node := r.localRuntime(), r.controller.Target().String()
	return (r.ctx == nil || r.ctx.Err() == nil) && root.runtimeFor(node) == r && r.controller.Connected() && root.resourceVisible(node)
}

// hookResources follows the same authoritative directory as project navigation
// when it contains a newer projection than the connected Client's snapshot.
func (r *Runtime) hookResources() ([]protocol.Project, []protocol.Worktree) {
	snapshot := r.snapshot()
	if catalog := r.localRuntime().resourceCatalog; catalog != nil {
		for _, source := range catalog.Sources {
			if source.Node == r.controller.Target() && source.Fault == nil && source.Revision >= snapshot.Cursor {
				return source.Projects, source.Worktrees
			}
		}
	}
	return snapshot.Projects, snapshot.Worktrees
}

func (r *Runtime) hookWorktree(id protocol.WorktreeID) (protocol.Worktree, bool) {
	if id == (protocol.WorktreeID{}) {
		return protocol.Worktree{}, false
	}
	_, trees := r.hookResources()
	for _, tree := range trees {
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
	id := r.workspaceSettings.worktree
	_, exists := r.hookWorktree(id)
	return r.workspaceSettings.hookTarget, exists && r.hookOwnerAvailable()
}

func (r *Runtime) hookTarget(tree protocol.Worktree, name string) settings.HookTarget {
	if name == "" {
		name = tree.Project.String()
	}
	return settings.HookTarget{Node: r.controller.Target().String(), Host: r.packageHostLabel(), Project: name, Worktree: tree.ID.String(), Path: tree.Path}
}

func (r *Runtime) workspaceHookTargets() []settings.HookTarget {
	var targets []settings.HookTarget
	for _, owner := range runtimeOwners(r) {
		if !owner.hookOwnerAvailable() {
			continue
		}
		projects, trees := owner.hookResources()
		names := make(map[protocol.ProjectID]string, len(projects))
		for _, project := range projects {
			names[project.ID] = project.Name
		}
		seen := make(map[protocol.WorktreeID]bool, len(trees))
		add := func(tree protocol.Worktree) {
			if tree.Project != nil && !seen[tree.ID] {
				targets = append(targets, owner.hookTarget(tree, names[*tree.Project]))
				seen[tree.ID] = true
			}
		}
		for _, tree := range trees {
			add(tree)
		}
		if owner.workspace != nil {
			for _, tree := range owner.workspace.admitted {
				add(tree)
			}
		}
	}
	slices.SortFunc(targets, func(a, b settings.HookTarget) int {
		return cmp.Or(cmp.Compare(a.Project, b.Project), cmp.Compare(a.Host, b.Host), cmp.Compare(a.Path, b.Path),
			cmp.Compare(a.Node, b.Node), cmp.Compare(a.Worktree, b.Worktree))
	})
	return targets
}

func (r *Runtime) selectHookTarget(target settings.HookTarget) bool {
	m := r.App.Settings
	if !r.hooksSettingsOpen() || m.HooksEditing() || target.Node == m.HookSelection.Node && target.Worktree == m.HookSelection.Worktree {
		return false
	}
	m.HookSelection = target
	r.refreshWorkspaceHooks()
	return true
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
	model.Pending = true
	r.publishWorkspaceSettings()
	r.execute(protocol.Command{Kind: "read_hooks", Data: protocol.CommandReadHooksPayload{Worktree: id}}, func(c *ui.Context, out protocol.Output, err error) {
		model.Pending = false
		defer r.publishWorkspaceSettings()
		if err != nil {
			r.reportKeys(c, err, "hooks_failed", "hooks_conflict", "hooks_unknown")
			return
		}
		value, ok := detail[protocol.HooksState](out.Data)
		if out.Kind != "hook_settings" || !ok || value.Worktree != id {
			r.reportKeys(c, fileActionFault(protocol.ErrorCodeInvalidRequest, "hook settings belong to another worktree"), "hooks_failed", "hooks_conflict", "hooks_unknown")
			return
		}
		draft := hookDraft(value.Settings)
		if reflect.DeepEqual(model.Draft, model.Saved) {
			model.Draft = draft
		}
		model.Saved, model.Revision = draft, value.Settings.Revision
		if id == r.workspaceSettings.worktree {
			r.workspaceSettings.sandboxed = value.Sandboxed
		}
	})
}
func (r *Runtime) saveWorkspaceHooks(id protocol.WorktreeID, draft settings.HookSettings, current func() bool) {
	m := r.workspaceHooksModel(id)
	if m.Pending {
		return
	}
	m.Draft, m.Pending = draft, true
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
	// Retry carries the same draft and revision while this binding is active.
	again := func() {
		if _, available := r.hookContext(); current() && available && reflect.DeepEqual(m.Draft, draft) {
			r.saveWorkspaceHooks(id, draft, current)
		}
	}
	r.saveSettingRetry(protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Worktree: id, Settings: value, ExpectedRevision: m.Revision}}, settings.Messages{Done: "hooks_saved", Failed: "hooks_failed", Conflict: "hooks_conflict", Uncertain: "hooks_unknown"}, nil, again, func(out protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishWorkspaceSettings()
		if !ok {
			return
		}
		state, valid := detail[protocol.HooksState](out.Data)
		if out.Kind != "hook_settings" || !valid || state.Worktree != id {
			return
		}
		m.Saved, m.Draft, m.Revision = hookDraft(state.Settings), hookDraft(state.Settings), state.Settings.Revision
		if id == r.workspaceSettings.worktree {
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
