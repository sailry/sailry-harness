package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var sandboxMessages = Messages{"sandbox_settings_saved", "sandbox_settings_failed", "sandbox_settings_conflict", "sandbox_settings_unknown"}

// conversation is the host's command sandbox, the sessions' order, and how
// messages show.
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
		})
	})
	kit.Group(c, "sessions", i18n.T(c, "settings_sessions"), "", nil, func() {
		kit.Card(c, func() {
			kit.SegmentedRow(c, "order", i18n.T(c, "settings_session_order"), "", &m.SessionOrder,
				i18n.T(c, "session_order_created"), i18n.T(c, "session_order_updated"), i18n.T(c, "session_order_name"))
		})
	})
	kit.Group(c, "messages", i18n.T(c, "settings_messages"), "", nil, func() {
		kit.Card(c, func() {
			kit.SegmentedRow(c, "display", i18n.T(c, "settings_message_display"), i18n.T(c, "settings_message_display_description"),
				&m.MessageDisplay, i18n.T(c, "message_display_detailed"), i18n.T(c, "message_display_compact"))
			title := i18n.T(c, "settings_message_text_size")
			kit.WideRow(c, "text-size", title, "", func() {
				// One tick a pixel, 12 to 18; the size chosen reads beside
				// the slider.
				value := i18n.T(c, "message_text_size_value", "size", int(m.TextSize))
				ui.StepSlider(c.Key("slider"), &m.TextSize, 12, 18, 1).Label(title).Grow(1).Shrink(1).MinWidth(0)
				ui.Text(c.Key("value"), value).FontSize(14).Font("monospace").SingleLine().Width(40).Shrink(0).TextAlign(ui.End).
					TextColor(theme.Of(c).MutedForeground)
			})
		})
	})
}
