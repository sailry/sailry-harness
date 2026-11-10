package settings

import (
	"path/filepath"
	"regexp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var terminalMessages = Messages{Failed: "terminal_settings_failed", Conflict: "terminal_settings_conflict", Uncertain: "terminal_settings_unknown"}

// TerminalMessages are the toasts of the terminal settings' outcomes.
var TerminalMessages = terminalMessages

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

// terminal is the host's environment and shell, paste protection, and the
// terminal's font.
func terminal(c *ui.Context, m *Model) {
	p := theme.Of(c)
	h := &m.Host
	e := &m.env
	locked := h.Pending || m.Live && m.SaveTerminal == nil
	kit.Group(c, "environment", i18n.T(c, "terminal_environment"), "", func() {
		label := i18n.T(c, "settings_add")
		kit.Primary(c.Key("add"), label, locked).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
			*e = envEditor{open: true, index: -1}
		})
	}, func() {
		if len(h.Draft.Env) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: i18n.T(c, "settings_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for i, v := range h.Draft.Env {
				kit.Entry(c, v.Key, func() { kit.Record(c, v.Key, v.Value) }, nil, func() {
					kit.Action(c.Key("edit"), icons.Settings2, i18n.T(c, "settings_edit"), locked).OnClick(func() {
						*e = envEditor{open: true, index: i, key: v.Key, value: v.Value}
					})
					kit.Action(c.Key("delete"), icons.CircleX, i18n.T(c, "settings_delete"), locked).OnClick(func() {
						h.Draft.Env = slices.Delete(slices.Clone(h.Draft.Env), i, i+1)
						saveTerminal(c, m)
					})
				})
			}
		})
	})
	kit.Group(c, "shell", i18n.T(c, "terminal_shell_group"), "", nil, func() {
		kit.Card(c, func() {
			names := shellTitles(c, m.Shells)
			title := i18n.T(c, "terminal_shell")
			kit.WideRow(c, "shell", title, "", func() {
				if pick(c, "select", title, &h.Draft.Shell, names, locked).Fill().Changed() {
					saveTerminal(c, m)
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
		saveTerminal(c, m)
		return ""
	})
}

// saveTerminal saves the host's draft: the Node's command when live, the
// preview's commit otherwise.
func saveTerminal(c *ui.Context, m *Model) {
	if !m.Live {
		report(c, m.Host.Commit(), terminalMessages)
	} else if m.SaveTerminal != nil {
		m.SaveTerminal()
	}
}

// TerminalEditing reports whether the environment variable dialog is open,
// so its draft is not replaced under it.
func (m *Model) TerminalEditing() bool { return m.env.open }

// shellTitles names the shells of the select, which tells its options
// apart by name: a program whose name another shares shows its whole
// command.
func shellTitles(c *ui.Context, shells []preview.Shell) []string {
	names := make([]string, len(shells))
	seen := map[string]int{}
	for i, s := range shells {
		names[i] = shellTitle(c, s)
		seen[names[i]]++
	}
	for i, s := range shells {
		if seen[names[i]] > 1 {
			names[i] = strings.Join(append([]string{s.Path}, s.Args...), " ")
		}
	}
	return names
}

// shellTitle names a shell: the host's default, or the program's name.
func shellTitle(c *ui.Context, s preview.Shell) string {
	if s.Path == "" {
		return i18n.T(c, "terminal_shell_default")
	}
	return filepath.Base(s.Path)
}
