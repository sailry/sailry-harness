package settings

import (
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

var hookMessages = Messages{"hooks_saved", "hooks_failed", "hooks_conflict", "hooks_unknown"}

// phases name the hook phases (hooks/view.rs key).
var phases = []string{"hooks_before_tool", "hooks_after_tool", "hooks_turn_end"}

// hookEditor is the hook dialog's draft (hooks/editor.rs): the script at
// index, or a new one at -1, and its timeout as typed.
type hookEditor struct {
	open    bool
	index   int
	script  preview.Hook
	timeout string
}

// consent is the enabling dialog (hooks/view.rs consent): the settings
// that run once the user agrees.
type consent struct {
	open     bool
	settings HookSettings
}

// Check validates a hook's draft: the i18n key of the problem, or "".
func (e *hookEditor) Check() string {
	ms, err := strconv.Atoi(strings.TrimSpace(e.timeout))
	if strings.TrimSpace(e.script.Command) == "" || err != nil || ms <= 0 {
		return "hooks_invalid"
	}
	e.script.TimeoutMS = ms
	return ""
}

// hooks is hooks/view.rs: hooks on or off, and the scripts, each with its
// switch, its editor and its removal. Turning scripts on asks for consent.
func hooks(c *ui.Context, m *Model) {
	h := &m.Hooks
	save := func(s HookSettings) {
		h.Draft = s
		report(c, h.Commit(), hookMessages)
	}
	enable := func(s HookSettings) {
		if s.Enabled && slices.ContainsFunc(s.Scripts, func(x preview.Hook) bool { return x.Enabled }) {
			m.consent = consent{open: true, settings: s}
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
		kit.Action(c.Key("add"), icons.Plus, i18n.T(c, "hooks_add"), h.Pending).OnClick(func() {
			m.hook = hookEditor{open: true, index: -1, script: preview.Hook{Phase: preview.BeforeTool}, timeout: "30000"}
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
						m.hook = hookEditor{open: true, index: i, script: x, timeout: strconv.Itoa(x.TimeoutMS)}
					})
					kit.Action(c.Key("remove"), icons.CircleX, i18n.T(c, "hooks_remove"), h.Pending).OnClick(func() {
						s := clone(h.Draft)
						s.Scripts = slices.Delete(s.Scripts, i, i+1)
						if len(s.Scripts) == 0 {
							s.Enabled = false
						}
						save(s)
					})
				})
			}
		})
	})
	hookDialog(c, m, save)
	consentDialog(c, m, save)
}

func clone(s HookSettings) HookSettings {
	s.Scripts = slices.Clone(s.Scripts)
	return s
}

// hookDialog is hooks/editor.rs: the hook's name, phase, command and
// timeout. The reference saves as it is edited and Close ends the edit;
// here Close saves the draft once it is valid, a problem toasting and
// keeping the dialog open.
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
				if pick(c, "select", label, &phase, labels(c, phases), false).Fill().Changed() {
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
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() {
				if problem := e.Check(); problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				s := clone(m.Hooks.Draft)
				if e.index < 0 {
					e.script.ID = fmt.Sprintf("hook-%d", len(s.Scripts)+1)
					s.Scripts = append(s.Scripts, e.script)
				} else {
					s.Scripts[e.index] = e.script
				}
				e.open = false
				save(s)
			})
		})
	}).Width(min(600, width(c)-48)).Label(title)
}

// consentDialog asks before scripts run (hooks/view.rs consent): what
// they may do, where, and each enabled script's phase, command and
// timeout.
func consentDialog(c *ui.Context, m *Model, save func(HookSettings)) {
	k := &m.consent
	if !k.open {
		return
	}
	p := theme.Of(c)
	title := i18n.T(c, "hooks_enable")
	kit.Dialog(c.Key("consent"), &k.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("details")).Grow(1).Shrink(1).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, "hooks_consent")).FontSize(14)
			ui.Text(c, i18n.T(c, "hooks_scope_workspace")).FontSize(14)
			ui.Text(c, i18n.T(c, "composer_host_local")+" · ~/Projects/sailry-go · main").FontSize(14).Selectable()
			for _, x := range k.settings.Scripts {
				if !x.Enabled {
					continue
				}
				ui.Column(c.Key(x.ID)).Gap(4).Children(func() {
					ui.Text(c, i18n.T(c, phases[x.Phase])).FontSize(14).TextColor(p.MutedForeground)
					ui.Text(c, x.Command).FontSize(14).Font("monospace").Selectable()
					ui.Textf(c, "%s: %d", i18n.T(c, "hooks_timeout"), x.TimeoutMS).FontSize(14)
				})
			}
		})
		footer(c, title, func() { k.open = false }, func() {
			k.open = false
			save(k.settings)
		})
	}).Width(min(600, width(c)-48)).Label(title)
}
