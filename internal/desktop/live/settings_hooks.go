package live

import (
	"reflect"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type hookSettingsState struct {
	model     settings.Stored[settings.HookSettings]
	loaded    bool
	reading   bool
	reload    bool
	sandboxed bool
	status    string
}

func (r *Runtime) BindHookSettings() settings.WindowBinding {
	m := r.App.Settings
	clear := func(status string) {
		m.HooksStatus = status
		m.HooksContext, m.SaveHooks, m.HooksSandboxed = nil, nil, nil
		m.Hooks = settings.Stored[settings.HookSettings]{}
		if status != "" {
			m.HooksContext = func() (settings.HookTarget, bool) {
				if selected := r.runtimeFor(m.ManagedNode); selected != nil {
					return selected.hooksTarget(), false
				}
				return settings.HookTarget{Node: m.ManagedNode}, false
			}
		}
	}
	return settings.WindowBinding{
		Administrator: true,
		KeepOnOutage:  true,
		Blocked:       clear,
		Mount: func(target settings.PageTarget) settings.PageBinding {
			owner := r.runtimeFor(target.Node)
			state := &owner.hookSettings
			state.loaded, state.reload, state.status = false, true, ""
			return settings.PageBinding{
				Follow: func() {
					if state.reload && !state.reading && !state.model.Pending {
						state.reload = false
						owner.readHooks(target)
					}
					owner.publishHookSettings()
				},
				Suspend: func(status string) {
					state.loaded, state.reload = false, true
					clear(status)
				},
				Close: func() { clear("") },
			}
		},
	}
}

func (r *Runtime) hooksTarget() settings.HookTarget {
	return settings.HookTarget{Node: r.controller.Target().String(), Host: r.packageHostLabel()}
}

func (r *Runtime) publishHookSettings() {
	page := r.currentSettingsPage(settings.Hooks)
	if page == nil {
		return
	}
	m, state := r.App.Settings, &r.hookSettings
	m.Hooks, m.HooksStatus = settings.Stored[settings.HookSettings]{}, state.status
	if state.loaded {
		m.Hooks = state.model
	}
	m.Hooks.Pending = state.model.Pending || state.reading || state.reload
	target := r.hooksTarget()
	revision := state.model.Revision
	m.HooksContext = func() (settings.HookTarget, bool) { return target, page.target.Current() }
	m.HooksSandboxed = func() bool { return state.sandboxed }
	m.SaveHooks = nil
	if state.loaded {
		m.SaveHooks = func(draft settings.HookSettings) bool {
			if !page.target.Current() || !state.loaded || state.reload || state.reading || state.model.Pending || state.model.Revision != revision {
				return false
			}
			r.saveHooks(draft)
			return true
		}
	}
}

func hookDraft(value protocol.HooksSettings) settings.HookSettings {
	draft := settings.HookSettings{Enabled: value.Enabled, Scripts: []viewmodel.Hook{}}
	add := func(id string, phase viewmodel.HookPhase, script *protocol.HooksScript) {
		if script != nil {
			draft.Scripts = append(draft.Scripts, viewmodel.Hook{ID: id, Name: script.Name, Command: script.Command, Phase: phase, Tools: slices.Clone(script.Tools), TimeoutMS: int(script.TimeoutMs), Enabled: script.Enabled})
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

func (r *Runtime) readHooks(target settings.PageTarget) {
	state := &r.hookSettings
	model := &state.model
	state.reading, state.status = true, ""
	r.async(func() {
		output, err := r.controller.Do(target.Context, protocol.Command{Kind: "read_hooks"})
		r.enqueue(func(c *ui.Context) {
			state.reading = false
			if !target.Current() || state.reload {
				return
			}
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
				r.reportKeys(c, err, state.status, "hooks_conflict", "hooks_unknown")
				return
			}
			draft := hookDraft(value.Settings)
			if reflect.DeepEqual(model.Draft, model.Saved) {
				model.Draft = draft
			}
			model.Saved, model.Revision = draft, value.Settings.Revision
			state.loaded, state.sandboxed = true, value.Sandboxed
		})
	})
}

func (r *Runtime) saveHooks(draft settings.HookSettings) {
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
		script := protocol.HooksScript{Name: entry.Name, Enabled: entry.Enabled, Command: entry.Command, Tools: slices.Clone(entry.Tools), TimeoutMs: uint64(entry.TimeoutMS)}
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
	command := protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: value, ExpectedRevision: m.Revision}}
	messages := settings.Messages{Done: "hooks_saved", Failed: "hooks_failed", Conflict: "hooks_conflict", Uncertain: "hooks_unknown"}
	r.saveSetting(command, messages, func(output protocol.Output, ok bool) {
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
