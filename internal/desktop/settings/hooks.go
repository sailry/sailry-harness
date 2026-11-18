package settings

import (
	"path"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
	target  HookTarget
	// removing is the ID of the script whose removal awaits confirmation.
	removing string
}

// consent is the enabling dialog: the settings that run once the user agrees.
type consent struct {
	open     bool
	settings HookSettings
	target   HookTarget
}

type HookTarget struct {
	Node, Host, Project, Worktree, Path string
}

func (t HookTarget) label(c *ui.Context) string {
	return strings.Join([]string{i18n.T(c, t.Host), t.Project, t.Path}, " · ")
}

func (t HookTarget) same(other HookTarget) bool {
	return t.Node == other.Node && t.Worktree == other.Worktree
}

func hooksTargetSelect(c *ui.Context, m *Model) bool {
	if !m.Live || m.HookTargets == nil || m.SelectHookTarget == nil {
		return false
	}
	target, available := m.HookSelection, false
	if m.HooksContext != nil {
		target, available = m.HooksContext()
	}
	targets := m.HookTargets()
	title := i18n.T(c, "hooks_project_required")
	shown, hint := title, title
	if target.Worktree != "" {
		worktree := path.Base(strings.ReplaceAll(target.Path, `\`, "/"))
		shown = target.Project + " · " + worktree + " · " + i18n.T(c, target.Host)
		hint = target.label(c)
		if !available {
			status := " · " + i18n.T(c, "unavailable")
			shown, hint = shown+status, hint+status
		}
	}
	changed := false
	button := kit.MenuButton(c, "hooks-target", func(b ui.Element) {
		kit.GhostLook(c, b.Label(title).Disabled(m.HooksEditing() || len(targets) == 0 && m.HookSelection.Node == ""), false).
			Row().Height(28).Gap(6).PaddingX(8).MaxWidth(320).Shrink(1).MinWidth(0).Children(func() {
			p := theme.Of(c)
			ui.Icon(c, icons.GitBranch).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
			ui.Text(c, shown).FontSize(13).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
		})
	}, func(menu *kit.Menu) {
		if menu.Item(i18n.T(c, "hooks_follow_current_worktree")).Checked(m.HookSelection.Node == "").Chosen() {
			changed = m.SelectHookTarget(HookTarget{})
		}
		if len(targets) > 0 {
			menu.Separator()
		}
		for _, choice := range targets {
			name := choice.Project + " · " + i18n.T(c, choice.Host)
			if menu.Item(name).Description(choice.Path).Icon(icons.GitBranch).Checked(m.HookSelection.same(choice)).Chosen() {
				changed = m.SelectHookTarget(choice)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd, CheckRight: true, MaxHeight: 320})
	kit.Tooltip(c, button, hint)
	return changed
}

func (m *Model) HooksEditing() bool {
	return m.hook.open || m.consent.open || m.hook.removing != ""
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
		if m.HooksContext == nil {
			return
		}
		var ok bool
		target, ok = m.HooksContext()
		if (m.hook.open || m.hook.removing != "") && !m.hook.target.same(target) {
			m.hook = hookEditor{}
		}
		if m.consent.open && !m.consent.target.same(target) {
			m.consent = consent{}
		}
		if !ok {
			if target.Worktree != "" {
				settingsTargetStatus(c, "unavailable")
			} else if m.HookTargets == nil || len(m.HookTargets()) == 0 {
				settingsTargetStatus(c, "connection_no_projects")
			}
			m.consent, m.hook.removing = consent{}, ""
			hookDialog(c, m, nil)
			return
		}
	}
	saveHooks := m.SaveHooks
	save := func(s HookSettings) {
		if m.Live {
			if saveHooks != nil {
				saveHooks(s)
			}
		} else {
			h.Draft = s
			report(c, h.Commit(), hookMessages)
		}
	}
	enable := func(s HookSettings) {
		if s.Enabled && slices.ContainsFunc(s.Scripts, func(x preview.Hook) bool { return x.Enabled }) {
			m.consent = consent{open: true, settings: s, target: target}
			return
		}
		save(s)
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
			m.hook = hookEditor{open: true, index: -1, script: preview.Hook{Phase: preview.BeforeTool}, timeout: "30000", target: target}
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
							save(s)
						}
					}
				}, func() {
					kit.Action(c.Key("edit"), icons.Settings2, i18n.T(c, "hooks_configure"), h.Pending).OnClick(func() {
						m.hook = hookEditor{open: true, index: i, script: x, timeout: strconv.Itoa(x.TimeoutMS), target: target}
					})
					kit.Action(c.Key("remove"), icons.CircleX, i18n.T(c, "hooks_remove"), h.Pending).OnClick(func() {
						m.hook.removing, m.hook.target = x.ID, target
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
			save(s)
			open = false
		}
		if !open {
			m.hook.removing = ""
		}
	} else {
		m.hook.removing = ""
	}
	hookDialog(c, m, save)
	consentDialog(c, m, save)
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
func hookDialog(c *ui.Context, m *Model, save func(HookSettings)) {
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
			if save == nil {
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
					kit.Toast(c, kit.Failed, i18n.T(c, "hooks_target_changed"))
					return
				}
				e.open = false
				save(s)
			})
		})
	}).Width(min(600, width(c)-48)).Label(title)
}

// consentDialog asks before scripts run: what they may do, where, and each
// enabled script's phase, command and timeout.
func consentDialog(c *ui.Context, m *Model, save func(HookSettings)) {
	k := &m.consent
	if !k.open {
		return
	}
	title := i18n.T(c, "hooks_enable")
	location := i18n.T(c, "composer_host_local") + " · ~/Projects/sailry-go · main"
	if m.Live && m.HooksContext != nil {
		target, ok := m.HooksContext()
		if !ok {
			return
		}
		location = target.label(c)
	}
	message := []string{i18n.T(c, "hooks_consent"), i18n.T(c, hookScope(m)), location}
	for _, x := range k.settings.Scripts {
		if x.Enabled {
			message = append(message, i18n.T(c, phases[x.Phase])+"\n"+x.Command+"\n"+i18n.T(c, "hooks_timeout")+": "+strconv.Itoa(x.TimeoutMS))
		}
	}
	if kit.AlertDialog(c.Key("consent"), &k.open, title, strings.Join(message, "\n\n"), i18n.T(c, "settings_cancel"), title) == 1 {
		save(k.settings)
	}
}
