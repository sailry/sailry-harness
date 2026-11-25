package testassembly

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
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var variableName = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*$`)

func terminal(c *ui.Context, state *settings.State) {
	m := state.Model
	host, editor := &m.Host, m.TerminalEnvEditor()
	locked := host.Pending || m.Live && m.SaveTerminal == nil
	save := func() {
		if !m.Live {
			settings.Report(c, host.Commit(), settings.TerminalMessages)
		} else if m.SaveTerminal != nil {
			m.SaveTerminal()
		}
	}
	kit.Group(c, "environment", i18n.T(c, "terminal_environment"), "", func() {
		kit.PrimaryBusy(c.Key("add"), i18n.T(c, "settings_add"), locked, m.Live && host.Pending).OnClick(func() {
			*editor = settings.TerminalEnvEditor{Open: true, Index: -1}
		})
	}, func() {
		if len(host.Draft.Env) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: i18n.T(c, "settings_empty"), Card: true})
			return
		}
		kit.Card(c, func() {
			for index, variable := range host.Draft.Env {
				kit.Entry(c, variable.Key, func() { kit.Record(c, variable.Key, variable.Value) }, nil, func() {
					kit.Action(c.Key("edit"), icons.Settings2, i18n.T(c, "settings_edit"), locked).OnClick(func() {
						*editor = settings.TerminalEnvEditor{Open: true, Index: index, Key: variable.Key, Value: variable.Value}
					})
					kit.Action(c.Key("delete"), icons.CircleX, i18n.T(c, "settings_delete"), locked).OnClick(func() {
						host.Draft.Env = slices.Delete(slices.Clone(host.Draft.Env), index, index+1)
						save()
					})
				})
			}
		})
	})
	kit.Group(c, "shell", i18n.T(c, "terminal_shell_group"), "", nil, func() {
		kit.Card(c, func() {
			names := make([]string, len(m.Shells))
			for index, shell := range m.Shells {
				names[index] = filepath.Base(shell.Path)
				if shell.Path == "" {
					names[index] = i18n.T(c, "terminal_shell_default")
				}
			}
			title := i18n.T(c, "terminal_shell")
			kit.WideRow(c, "shell", title, "", func() {
				if settings.Pick(c, "select", title, &host.Draft.Shell, names, locked).Fill().Changed() {
					save()
				}
			})
		})
	})
	settings.DraftDialog(c, "environment-editor", &editor.Open, "terminal_environment_add", []settings.DraftField{
		{Label: "terminal_variable", Hint: "form_variable_hint", Value: &editor.Key},
		{Label: "terminal_value", Hint: "form_value_hint", Value: &editor.Value},
	}, func() string {
		key := strings.TrimSpace(editor.Key)
		if !variableName.MatchString(key) {
			return "terminal_variable_invalid"
		}
		for index, variable := range host.Draft.Env {
			if index != editor.Index && variable.Key == key {
				return "terminal_variable_duplicate"
			}
		}
		variable := preview.EnvVar{Key: key, Value: editor.Value}
		host.Draft.Env = slices.Clone(host.Draft.Env)
		if editor.Index < 0 {
			host.Draft.Env = append(host.Draft.Env, variable)
		} else {
			host.Draft.Env[editor.Index] = variable
		}
		save()
		return ""
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
					ui.Slider(c.Key("slider"), &m.FontSize, 8, 32).Step(1).Label(title).Grow(1)
					m.FontSize = float64(int(m.FontSize + 0.5))
					ui.Textf(c, "%d", int(m.FontSize)).FontSize(14).TextColor(theme.Of(c).SurfaceForeground)
				})
			})
			kit.WideRow(c, "font", i18n.T(c, "settings_font_family"), "", func() {
				options := []kit.SelectOption{{Value: "", Label: i18n.T(c, "settings_font_default")}, {Value: "Courier New", Label: "Courier New"}}
				if m.Font != "" && m.Font != "Courier New" {
					options = append(options, kit.SelectOption{Value: m.Font, Label: m.Font})
				}
				element, _ := kit.SelectField(c, "font", &m.Font, kit.SelectProps{Label: i18n.T(c, "settings_font_family"), Options: options})
				element.FillWidth().MinWidth(0)
			})
		})
	})
}
