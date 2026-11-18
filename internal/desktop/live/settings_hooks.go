package live

import (
	"reflect"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type hookSettingsState struct {
	model     settings.Stored[settings.HookSettings]
	loaded    bool
	reload    bool
	sandboxed bool
	status    string
	// The selected binding survives temporary loss of the connection. The
	// shown binding reopens with a fresh read when the connection returns.
	selected   *Runtime
	shown      *Runtime
	generation uint64
}

func (r *Runtime) hooksSettingsOpen() bool {
	return r.App.Page == shell.SettingsPage && r.App.Settings.Section == settings.Hooks
}

func (r *Runtime) hooksSettingsFrame() {
	root := r.localRuntime()
	m := root.App.Settings
	var selected, owner *Runtime
	status := ""
	visible := root.hooksSettingsOpen()
	if visible {
		selected = root.runtimeFor(m.ManagedNode)
		status = selected.machineSettingsStatus()
		if status == "" {
			if selected.controller.Connected() {
				owner = selected
			} else {
				status = "connections_offline"
			}
		}
	}
	binding := &root.hookSettings
	if binding.selected != selected {
		binding.selected = selected
		binding.generation++
	}
	if binding.shown != owner {
		binding.shown = owner
		if owner != nil {
			owner.hookSettings.reload = true
		}
	}
	if owner == nil {
		m.HooksStatus = status
		m.HooksContext, m.SaveHooks, m.HooksSandboxed = nil, nil, nil
		if visible {
			m.Hooks = settings.Stored[settings.HookSettings]{}
			target := settings.HookTarget{Node: m.ManagedNode}
			if selected != nil {
				target = selected.hooksTarget()
			}
			m.HooksContext = func() (settings.HookTarget, bool) { return target, false }
		}
		return
	}
	if state := &owner.hookSettings; state.reload && !state.model.Pending {
		state.reload = false
		owner.readHooks()
	}
	owner.publishHookSettings()
}

func (r *Runtime) hooksTarget() settings.HookTarget {
	return settings.HookTarget{Node: r.controller.Target().String(), Host: r.packageHostLabel()}
}

func (r *Runtime) hooksSettingsCurrent() bool {
	root := r.localRuntime()
	return root.hookSettings.shown == r && root.hooksSettingsOpen() && root.managedSettingsNode() == r &&
		r.controller.Connected() && r.machineSettingsStatus() == ""
}

func (r *Runtime) hooksUser() protocol.UserID {
	if user := r.snapshot().User; user != nil {
		return user.ID
	}
	return protocol.UserID{}
}

func (r *Runtime) publishHookSettings() {
	if !r.hooksSettingsCurrent() {
		return
	}
	root := r.localRuntime()
	m, state := root.App.Settings, &r.hookSettings
	m.Hooks, m.HooksStatus = state.model, state.status
	target := r.hooksTarget()
	user := r.hooksUser()
	generation, revision := root.hookSettings.generation, state.model.Revision
	current := func() bool {
		return root.hookSettings.generation == generation && r.hooksSettingsCurrent() && r.hooksUser() == user
	}
	m.HooksContext = func() (settings.HookTarget, bool) { return target, current() }
	m.HooksSandboxed = func() bool { return state.sandboxed }
	m.SaveHooks = nil
	if state.loaded {
		canSave := func() bool { return current() && state.loaded && !state.reload && state.model.Revision == revision }
		m.SaveHooks = func(draft settings.HookSettings) bool {
			if !canSave() || state.model.Pending {
				return false
			}
			r.saveHooks(draft, canSave)
			return true
		}
	}
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

func (r *Runtime) readHooks() {
	state := &r.hookSettings
	model := &state.model
	model.Pending = true
	state.status = ""
	r.execute(protocol.Command{Kind: "read_hooks"}, func(c *ui.Context, output protocol.Output, err error) {
		model.Pending = false
		defer r.publishHookSettings()
		value, valid := detail[protocol.HooksState](output.Data)
		if err == nil && (output.Kind != "hook_settings" || !valid) {
			err = fileActionFault(protocol.ErrorCodeInvalidRequest, "unexpected host hooks response")
		}
		if err != nil {
			state.loaded, state.status = false, "unavailable"
			if faultCode(err) == protocol.ErrorCodePermissionDenied {
				state.status = "permission_denied"
			}
			if r.hooksSettingsCurrent() {
				r.reportKeys(c, err, state.status, "hooks_conflict", "hooks_unknown")
			}
			return
		}
		draft := hookDraft(value.Settings)
		if reflect.DeepEqual(model.Draft, model.Saved) {
			model.Draft = draft
		}
		model.Saved, model.Revision = draft, value.Settings.Revision
		state.loaded, state.sandboxed = true, value.Sandboxed
	})
}

func (r *Runtime) saveHooks(draft settings.HookSettings, current func() bool) {
	state := &r.hookSettings
	m := &state.model
	if m.Pending {
		return
	}
	m.Draft, m.Pending = draft, true
	r.publishHookSettings()
	value := protocol.DefaultHooksSettings()
	value.Revision, value.Enabled = m.Revision, draft.Enabled
	for _, entry := range draft.Scripts {
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
	again := func() {
		if current() && reflect.DeepEqual(m.Draft, draft) {
			r.saveHooks(draft, current)
		}
	}
	command := protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: value, ExpectedRevision: m.Revision}}
	messages := settings.Messages{Done: "hooks_saved", Failed: "hooks_failed", Conflict: "hooks_conflict", Uncertain: "hooks_unknown"}
	r.saveSettingRetry(command, messages, nil, again, func(output protocol.Output, ok bool) {
		m.Pending = false
		defer r.publishHookSettings()
		value, valid := detail[protocol.HooksState](output.Data)
		if !ok || output.Kind != "hook_settings" || !valid {
			return
		}
		saved := hookDraft(value.Settings)
		m.Saved, m.Draft, m.Revision = saved, saved, value.Settings.Revision
		state.sandboxed = value.Sandboxed
	})
}
