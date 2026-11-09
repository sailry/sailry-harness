package settings

import (
	"path/filepath"
	"regexp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

var terminalMessages = Messages{Failed: "terminal_settings_failed", Conflict: "terminal_settings_conflict", Uncertain: "terminal_settings_unknown"}

// envEditor is the environment variable dialog's draft: the variable at
// index, or a new one at -1.
type envEditor struct {
	open       bool
	index      int
	key, value string
}

// variable is terminal::validate_key's rule: a letter or underscore, then
// letters, digits and underscores.
var variable = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*$`)

// SetVariable saves an environment variable at index (-1 adds it): the
// i18n key of the problem, or "" once saved.
func (h *TerminalHost) SetVariable(index int, key, value string) string {
	key = strings.TrimSpace(key)
	if !variable.MatchString(key) {
		return "terminal_variable_invalid"
	}
	for i, e := range h.Env {
		if i != index && e.Key == key {
			return "terminal_variable_duplicate"
		}
	}
	if index < 0 {
		h.Env = append(h.Env, preview.EnvVar{Key: key, Value: value})
	} else {
		h.Env[index] = preview.EnvVar{Key: key, Value: value}
	}
	return ""
}

// terminal is settings/terminal.rs: the host's environment and shell,
// paste protection, and the terminal's font.
func terminal(c *ui.Context, m *Model) {
	p := theme.Of(c)
	h := &m.Host
	e := &m.env
	kit.Group(c, "environment", i18n.T(c, "terminal_environment"), "", func() {
		label := i18n.T(c, "settings_add")
		kit.Primary(c.Key("add"), label, h.Pending).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
			*e = envEditor{open: true, index: -1}
		})
	}, func() {
		if len(h.Draft.Env) == 0 {
			kit.EmptyGroup(c, icons.Inbox, i18n.T(c, "settings_empty"))
			return
		}
		kit.Card(c, func() {
			for i, v := range h.Draft.Env {
				kit.Entry(c, v.Key, func() { kit.Record(c, v.Key, v.Value) }, nil, func() {
					kit.Action(c.Key("edit"), icons.Settings2, i18n.T(c, "settings_edit"), h.Pending).OnClick(func() {
						*e = envEditor{open: true, index: i, key: v.Key, value: v.Value}
					})
					kit.Action(c.Key("delete"), icons.CircleX, i18n.T(c, "settings_delete"), h.Pending).OnClick(func() {
						h.Draft.Env = slices.Delete(slices.Clone(h.Draft.Env), i, i+1)
						report(c, h.Commit(), terminalMessages)
					})
				})
			}
		})
	})
	kit.Group(c, "shell", i18n.T(c, "terminal_shell_group"), "", nil, func() {
		kit.Card(c, func() {
			var names []string
			for _, s := range m.Shells {
				names = append(names, shellTitle(c, s))
			}
			title := i18n.T(c, "terminal_shell")
			kit.WideRow(c, "shell", title, "", func() {
				if pick(c, "select", title, &h.Draft.Shell, names, h.Pending).Fill().Changed() {
					report(c, h.Commit(), terminalMessages)
				}
			})
		})
	})
	kit.Group(c, "input", i18n.T(c, "terminal_input"), "", nil, func() {
		kit.Card(c, func() {
			kit.SwitchRow(c, "paste", i18n.T(c, "settings_paste_protection"), i18n.T(c, "settings_paste_description"), &m.PasteProtection)
		})
	})
	kit.Group(c, "display", i18n.T(c, "settings_terminal_display"), "", nil, func() {
		kit.Card(c, func() {
			title := i18n.T(c, "settings_font_size")
			kit.WideRow(c, "size", title, "", func() {
				ui.Row(c).FillWidth().Gap(12).Children(func() {
					ui.Slider(c.Key("slider"), &m.FontSize, 8, 32).Label(title).Grow(1)
					m.FontSize = float64(int(m.FontSize + 0.5))
					ui.Textf(c, "%d", int(m.FontSize)).FontSize(14).TextColor(p.SurfaceForeground)
				})
			})
			title = i18n.T(c, "settings_font_family")
			kit.WideRow(c, "font", title, "", func() {
				ui.TextInput(c.Key("input"), &m.Font).Label(title).Placeholder("Menlo").FillWidth()
			})
		})
	})
	draftDialog(c, "env", &e.open, "terminal_environment_add", []field{
		{label: "terminal_variable", hint: "form_variable_hint", value: &e.key},
		{label: "terminal_value", hint: "form_value_hint", value: &e.value},
	}, func() string {
		draft := h.Draft
		draft.Env = slices.Clone(draft.Env)
		if problem := draft.SetVariable(e.index, e.key, e.value); problem != "" {
			return problem
		}
		h.Draft = draft
		report(c, h.Commit(), terminalMessages)
		return ""
	})
}

// shellTitle names a shell as terminal/shell.rs title does: the host's
// default, or the program's name.
func shellTitle(c *ui.Context, s preview.Shell) string {
	if s.Path == "" {
		return i18n.T(c, "terminal_shell_default")
	}
	return filepath.Base(s.Path)
}
