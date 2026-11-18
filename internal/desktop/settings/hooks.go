package settings

import (
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var hookMessages = Messages{"hooks_saved", "hooks_failed", "hooks_conflict", "hooks_unknown"}

// phases name the hook phases.
var phases = []string{"hooks_before_tool", "hooks_after_tool", "hooks_turn_end"}

// hookEditor is the hook dialog's draft: the script at index, or a new one at
// -1, and its timeout as typed.
type hookEditor struct {
	open    bool
	index   int
	script  preview.Hook
	timeout string
	binding hookBinding
	// removing is the ID of the script whose removal awaits confirmation.
	removing string
}

// consent is the enabling dialog: the settings that run once the user agrees.
type consent struct {
	open     bool
	settings HookSettings
	binding  hookBinding
}

type HookTarget struct {
	Node, Host string
}

// hookBinding keeps the original revision and save action across frames.
type hookBinding struct {
	target   HookTarget
	revision uint64
	save     func(HookSettings) bool
}

func (binding hookBinding) submit(c *ui.Context, m *Model, draft HookSettings) bool {
	if !m.Live {
		m.Hooks.Draft = draft
		report(c, m.Hooks.Commit(), hookMessages)
		return true
	}
	var target HookTarget
	var available bool
	if m.HooksContext != nil {
		target, available = m.HooksContext()
	}
	if !available || m.HooksStatus != "" || m.Hooks.Pending || binding.save == nil ||
		binding.target.Node != target.Node || binding.revision != m.Hooks.Revision || !binding.save(draft) {
		kit.Toast(c, kit.Conflict, i18n.T(c, "hooks_conflict"))
		return false
	}
	return true
}

// Check validates a hook's draft: the i18n key of the problem, or "".
func (e *hookEditor) Check() string {
	ms, err := strconv.Atoi(strings.TrimSpace(e.timeout))
	if strings.TrimSpace(e.script.Command) == "" || err != nil || ms <= 0 || ms > 60000 {
		return "hooks_invalid"
	}
	e.script.TimeoutMS = ms
	return ""
}

// hooks is hooks on or off, and the scripts, each with its switch, its editor
// and its removal. Turning scripts on asks for consent.
func hooks(c *ui.Context, m *Model) {
	h := &m.Hooks
	var target HookTarget
	if m.Live {
		var available bool
		if m.HooksContext != nil {
			target, available = m.HooksContext()
		}
		if (m.hook.open || m.hook.removing != "") && m.hook.binding.target.Node != target.Node {
			m.hook = hookEditor{}
		}
		if m.consent.open && m.consent.binding.target.Node != target.Node {
			m.consent = consent{}
		}
		if !available || m.HooksStatus != "" {
			status := m.HooksStatus
			if status == "" {
				status = "unavailable"
			}
			settingsTargetStatus(c, status)
			m.consent, m.hook.removing = consent{}, ""
			hookDialog(c, m, false)
			return
		}
	}
	binding := hookBinding{target: target, revision: h.Revision, save: m.SaveHooks}
	enable := func(s HookSettings) {
		if s.Enabled && slices.ContainsFunc(s.Scripts, func(x preview.Hook) bool { return x.Enabled }) {
			m.consent = consent{open: true, settings: s, binding: binding}
			return
		}
		binding.submit(c, m, s)
	}
	kit.Group(c, "settings", i18n.T(c, "hooks_settings"), "", nil, func() {
		kit.Card(c, func() {
			on := h.Draft.Enabled
			kit.Row(c, "enabled", i18n.T(c, "hooks_enabled"), "", func() {
				if ui.Switch(c.Key("switch"), &on).Label(i18n.T(c, "hooks_enabled")).Disabled(h.Pending || len(h.Draft.Scripts) == 0).Changed() {
					s := clone(h.Draft)
					s.Enabled = on
					enable(s)
				}
			})
		})
	})
	kit.Group(c, "scripts", i18n.T(c, "hooks_scripts"), "", func() {
		if m.Live && h.Pending {
			kit.BusyAction(c.Key("add"), i18n.T(c, "hooks_add")) // the hooks are being read or saved
			return
		}
		kit.Action(c.Key("add"), icons.Plus, i18n.T(c, "hooks_add"), h.Pending).OnClick(func() {
			m.hook = hookEditor{open: true, index: -1, script: preview.Hook{Phase: preview.BeforeTool}, timeout: "30000", binding: binding}
		})
	}, func() {
		if len(h.Draft.Scripts) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Settings2, Title: i18n.T(c, "hooks_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for i, x := range h.Draft.Scripts {
				title, detail := x.Name, i18n.T(c, phases[x.Phase])
				if title == "" {
					title, detail = detail, x.Command
				}
				kit.Entry(c, x.ID, func() { kit.Record(c, title, detail) }, func() {
					on := x.Enabled
					if ui.Switch(c.Key("switch"), &on).Label(i18n.T(c, "hooks_enabled") + " " + title).Disabled(h.Pending).Changed() {
						s := clone(h.Draft)
						s.Scripts[i].Enabled = on
						if on {
							enable(s)
						} else {
							binding.submit(c, m, s)
						}
					}
				}, func() {
					kit.Action(c.Key("edit"), icons.Settings2, i18n.T(c, "hooks_configure"), h.Pending).OnClick(func() {
						m.hook = hookEditor{open: true, index: i, script: x, timeout: strconv.Itoa(x.TimeoutMS), binding: binding}
					})
					kit.Action(c.Key("remove"), icons.CircleX, i18n.T(c, "hooks_remove"), h.Pending).OnClick(func() {
						m.hook.removing, m.hook.binding = x.ID, binding
					})
				})
			}
		})
	})
	if i := slices.IndexFunc(h.Draft.Scripts, func(x preview.Hook) bool { return x.ID == m.hook.removing }); m.hook.removing != "" && i >= 0 {
		x := h.Draft.Scripts[i]
		name := x.Name
		if name == "" {
			name = x.Command
		}
		open := true
		if confirm(c, "remove-hook", &open, i18n.T(c, "hooks_remove"), name, i18n.T(c, "hooks_remove")) {
			s := clone(h.Draft)
			s.Scripts = slices.Delete(s.Scripts, i, i+1)
			if len(s.Scripts) == 0 {
				s.Enabled = false
			}
			m.hook.binding.submit(c, m, s)
			open = false
		}
		if !open {
			m.hook.removing = ""
		}
	} else {
		m.hook.removing = ""
	}
	hookDialog(c, m, !m.Live || m.SaveHooks != nil && !h.Pending)
	consentDialog(c, m)
}

func clone(s HookSettings) HookSettings {
	s.Scripts = slices.Clone(s.Scripts)
	return s
}

func hookScope(m *Model) string {
	if m.Live && m.HooksSandboxed != nil && !m.HooksSandboxed() {
		return "hooks_scope_windows"
	}
	return "hooks_scope_workspace"
}

// hookDialog is the hook's name, phase, command and timeout. The reference
// saves as it is edited and Close ends the edit; here Close saves the draft
// once it is valid, a problem toasting and keeping the dialog open.
func hookDialog(c *ui.Context, m *Model, canSave bool) {
	e := &m.hook
	if !e.open {
		return
	}
	title := i18n.T(c, "hooks_configure")
	kit.Dialog(c.Key("hook"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			label := i18n.T(c, "hooks_name")
			kit.Field(c, "name", label, "", func() {
				ui.TextInput(c.Key("input"), &e.script.Name).Label(label).Placeholder(i18n.T(c, "form_name_hint")).FillWidth()
			})
			label = i18n.T(c, "hooks_phase")
			kit.Field(c, "phase", label, "", func() {
				phase := int(e.script.Phase)
				if pick(c, "select", label, &phase, labels(c, phases), e.script.ID == "before_tool" || e.script.ID == "after_tool" || e.script.ID == "turn_end").Fill().Changed() {
					e.script.Phase = preview.HookPhase(phase)
				}
			})
			label = i18n.T(c, "hooks_command")
			kit.Field(c, "command", label, "", func() {
				ui.TextInput(c.Key("input"), &e.script.Command).Label(label).FillWidth().Font("monospace")
			})
			label = i18n.T(c, "hooks_timeout")
			kit.Field(c, "timeout", label, "", func() {
				ui.TextInput(c.Key("input"), &e.timeout).Label(label).FillWidth()
			})
		})
		ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
			if !canSave {
				kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { e.open = false })
				return
			}
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() {
				if problem := e.Check(); problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				s := clone(m.Hooks.Draft)
				if e.index < 0 {
					e.script.ID = protocol.NewRequestID().String()
					s.Scripts = append(s.Scripts, e.script)
				} else if index := slices.IndexFunc(s.Scripts, func(x preview.Hook) bool { return x.ID == e.script.ID }); index >= 0 {
					s.Scripts[index] = e.script
				} else {
					kit.Toast(c, kit.Conflict, i18n.T(c, "hooks_conflict"))
					return
				}
				if e.binding.submit(c, m, s) {
					e.open = false
				}
			})
		})
	}).Width(min(600, width(c)-48)).Label(title)
}

// consentDialog asks before scripts run: what they may do, where, and each
// enabled script's phase, command and timeout.
func consentDialog(c *ui.Context, m *Model) {
	k := &m.consent
	if !k.open {
		return
	}
	title := i18n.T(c, "hooks_enable")
	host := k.binding.target.Host
	if host == "" {
		host = "composer_host_local"
	}
	message := []string{i18n.T(c, "hooks_consent", "host", i18n.T(c, host)), i18n.T(c, hookScope(m))}
	for _, x := range k.settings.Scripts {
		if x.Enabled {
			message = append(message, i18n.T(c, phases[x.Phase])+"\n"+x.Command+"\n"+i18n.T(c, "hooks_timeout")+": "+strconv.Itoa(x.TimeoutMS))
		}
	}
	if kit.AlertDialog(c.Key("consent"), &k.open, title, strings.Join(message, "\n\n"), i18n.T(c, "settings_cancel"), title) == 1 {
		k.binding.submit(c, m, k.settings)
	}
}
