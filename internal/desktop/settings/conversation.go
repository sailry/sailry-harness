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

// conversation is the host's command sandbox and code index, the sessions'
// order, and how messages show.
func conversation(c *ui.Context, m *Model) {
	kit.Group(c, "execution", i18n.T(c, "settings_execution"), "", nil, func() {
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
		})
	})
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
