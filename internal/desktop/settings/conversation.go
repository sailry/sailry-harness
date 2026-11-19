package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

var sandboxMessages = Messages{"sandbox_settings_saved", "sandbox_settings_failed", "sandbox_settings_conflict", "sandbox_settings_unknown"}

// CodeIndexMessages are the code index switch's outcomes, shared with the
// live action adapter.
var CodeIndexMessages = Messages{"code_index_settings_saved", "code_index_settings_failed", "code_index_settings_conflict", "code_index_settings_unknown"}

// PersonalInstructions holds the editor draft for one settings host.
type PersonalInstructions struct {
	Saved, Draft              string
	Known, Pending, Uncertain bool
}

func conversation(c *ui.Context, m *Model) {
	kit.Group(c, "execution", i18n.T(c, "settings_execution"), "", nil, func() {
		if m.ExecutionSettingsStatus != "" {
			settingsTargetStatus(c, m.ExecutionSettingsStatus)
			return
		}
		kit.Card(c, func() {
			if settingSwitch(c, "sandbox", i18n.T(c, "settings_command_sandbox"), i18n.T(c, "settings_command_sandbox_description"), &m.Sandbox.Draft, m.Live && m.SaveSandbox == nil || m.Sandbox.Pending) {
				if m.Live {
					m.SaveSandbox()
				} else {
					report(c, m.Sandbox.Commit(), sandboxMessages)
				}
			}
			if settingSwitch(c, "code-index", i18n.T(c, "settings_code_index"), i18n.T(c, "settings_code_index_description"), &m.CodeIndex.Draft, m.Live && m.SaveCodeIndex == nil || m.CodeIndex.Pending) {
				if m.Live {
					m.SaveCodeIndex()
				} else {
					report(c, m.CodeIndex.Commit(), CodeIndexMessages)
				}
			}
			persistentCodeIndex(c, m)
		})
	})
	personalInstructions(c, m)
	kit.Group(c, "messages", i18n.T(c, "settings_messages"), "", nil, func() {
		kit.Card(c, func() {
			kit.SegmentedRow(c, "display", i18n.T(c, "settings_message_display"), i18n.T(c, "settings_message_display_description"),
				&m.MessageDisplay, i18n.T(c, "message_display_detailed"), i18n.T(c, "message_display_compact"))
			title := i18n.T(c, "settings_message_text_size")
			kit.WideRow(c, "text-size", title, "", func() {
				// One tick a pixel, 12 to 18.
				ui.StepSlider(c.Key("slider"), &m.TextSize, 12, 18, 1).Label(title).Grow(1).Shrink(1).MinWidth(0)
			})
		})
	})
}

func persistentCodeIndex(c *ui.Context, m *Model) {
	if m.PersistentCodeIndex == nil {
		return
	}
	enabled := *m.PersistentCodeIndex
	disabled := m.CodeIndex.Pending || !m.CodeIndex.Saved || m.SavePersistentCodeIndex == nil
	if settingSwitch(c, "persistent-code-index", i18n.T(c, "settings_code_index_persistent"), i18n.T(c, "settings_code_index_persistent_description"), &enabled, disabled) {
		m.SavePersistentCodeIndex(enabled)
	}
}

func personalInstructions(c *ui.Context, m *Model) {
	if m.ExecutionSettingsStatus != "" {
		return
	}
	e := m.PersonalInstructions
	label := i18n.T(c, "settings_custom_prompt")
	kit.Group(c, "personal-instructions", label, i18n.T(c, "settings_custom_prompt_description"), func() {
		if e != nil && !e.Known && !e.Pending && m.ReadPersonalInstructions != nil {
			kit.Secondary(c.Key("reload"), i18n.T(c, "settings_retry")).OnClick(m.ReadPersonalInstructions)
			return
		}
		action := "settings_save"
		if e != nil && e.Uncertain {
			action = "files_check_result"
		}
		disabled := e == nil || !e.Known || !e.Uncertain && e.Saved == e.Draft || m.Live && m.SavePersonalInstructions == nil
		kit.PrimaryBusy(c.Key("save"), i18n.T(c, action), disabled, e != nil && e.Pending).OnClick(func() {
			if m.Live {
				m.SavePersonalInstructions()
				return
			}
			e.Saved = e.Draft
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_saved"))
		})
	}, func() {
		if e == nil {
			return
		}
		ui.TextArea(c.Key(m.ManagedNode), &e.Draft).Label(label).Placeholder(i18n.T(c, "settings_custom_prompt_placeholder")).
			Disabled(!e.Known || e.Pending || e.Uncertain).FillWidth().Height(100)
	})
}
