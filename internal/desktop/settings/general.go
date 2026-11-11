package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var (
	powerMessages     = Messages{"power_settings_saved", "power_settings_failed", "power_settings_conflict", "power_settings_unknown"}
	telemetryMessages = Messages{"telemetry_saved", "telemetry_failed", "telemetry_conflict", "telemetry_unknown"}
)

// PowerMessages and TelemetryMessages are shared with the live action adapter.
var PowerMessages = powerMessages
var TelemetryMessages = telemetryMessages

// notifications are the notification switches, in Notify's order.
var notifications = []string{"settings_notify_system", "settings_notify_approval", "settings_notify_completed", "settings_notify_failed"}

// general is the language, the power and sidebar switches, the notifications
// and the diagnostics.
func general(c *ui.Context, m *Model) {
	kit.Group(c, "general", "", "", nil, func() {
		kit.Card(c, func() {
			// The window takes the language in the frame that saves it, so
			// the choice has no toast.
			choose(c, "language", i18n.T(c, "settings_language"), "", &m.Language, labels(c, Languages), 192)
			if settingSwitch(c, "power", i18n.T(c, "power_prevent_sleep"), i18n.T(c, "power_prevent_sleep_description"), &m.Power.Draft, m.Power.Pending) {
				if m.SavePower != nil {
					m.SavePower()
				} else {
					report(c, m.Power.Commit(), powerMessages)
				}
			}
		})
	})
	kit.Group(c, "notifications", i18n.T(c, "settings_notifications"), "", nil, func() {
		kit.Card(c, func() {
			for i, key := range notifications {
				kit.SwitchRow(c, key, i18n.T(c, key), i18n.T(c, key+"_description"), &m.Notify[i])
			}
			var seconds []string
			for _, s := range ToastSeconds {
				seconds = append(seconds, i18n.T(c, "settings_seconds", "count", s))
			}
			choose(c, "toast", i18n.T(c, "settings_toast_duration"), i18n.T(c, "settings_toast_description"), &m.Toast, seconds, 128)
		})
	})
	telemetry(c, m)
}

// telemetry is what diagnostics send, the identity reset and sending
// diagnostics, which the preview does not.
func telemetry(c *ui.Context, m *Model) {
	p := theme.Of(c)
	t := &m.Telemetry
	kit.Group(c, "telemetry", i18n.T(c, "telemetry_title"), "", nil, func() {
		kit.Card(c, func() {
			if settingSwitch(c, "errors", i18n.T(c, "telemetry_errors"), i18n.T(c, "telemetry_errors_description"), &t.Draft.Errors, t.Pending) {
				if m.SaveTelemetry != nil {
					m.SaveTelemetry()
				} else {
					report(c, t.Commit(), telemetryMessages)
				}
			}
			if settingSwitch(c, "metrics", i18n.T(c, "telemetry_metrics"), i18n.T(c, "telemetry_metrics_description"), &t.Draft.Metrics, t.Pending) {
				if m.SaveTelemetry != nil {
					m.SaveTelemetry()
				} else {
					report(c, t.Commit(), telemetryMessages)
				}
			}
			kit.Row(c, "identity", i18n.T(c, "telemetry_identity"), "", func() {
				kit.SecondaryBusy(c.Key("reset"), i18n.T(c, "telemetry_reset_action"), m.Busy[BusyTelemetry]).Height(28).PaddingX(10).Disabled(m.Live && (m.ResetTelemetry == nil || t.Pending)).OnClick(func() {
					if m.ResetTelemetry != nil {
						m.ResetTelemetry()
					} else {
						notice(c)
					}
				})
			})
			kit.Row(c, "support", i18n.T(c, "telemetry_support"), "", func() {
				kit.SecondaryBusy(c.Key("send"), i18n.T(c, "telemetry_send_action"), m.Busy[BusyDiagnostics]).Height(28).PaddingX(10).Disabled(m.Live && (m.SendDiagnostics == nil || t.Pending)).OnClick(func() {
					if m.SendDiagnostics != nil {
						m.SendDiagnostics()
					} else {
						notice(c)
					}
				})
			})
			ui.Text(c, i18n.T(c, "telemetry_excludes")).FontSize(14).Padding(12, 0).TextColor(p.MutedForeground)
		})
	})
}

// small is Kit's small outline button.
func small(c *ui.Context, key any, label string) ui.Element {
	return kit.Secondary(c.Key(key), label).Height(28).PaddingX(10)
}

// Kit's switch row has no disabled parameter. Keep its composition while a
// revisioned save owns the draft, so another toggle cannot race that save.
func settingSwitch(c *ui.Context, key any, title, description string, on *bool, pending bool) bool {
	changed := false
	kit.Row(c, key, title, description, func() { changed = ui.Switch(c.Key("switch"), on).Label(title).Disabled(pending).Changed() })
	return changed
}

// labels translates i18n keys.
func labels(c *ui.Context, keys []string) []string {
	out := make([]string, len(keys))
	for i, k := range keys {
		out[i] = i18n.T(c, k)
	}
	return out
}

// choose is a Row with a Select of options by index; width is the
// select's (Row's div().w_48() and the like). It reports a new choice.
func choose(c *ui.Context, key any, title, description string, chosen *int, options []string, width float32) bool {
	changed := false
	kit.Row(c, key, title, description, func() {
		changed = pick(c, key, title, chosen, options, false).Width(width).Changed()
	})
	return changed
}

// pick is a Select of options by index. Its Changed reports a new choice
// through the element it returns (see picked).
func pick(c *ui.Context, key any, title string, chosen *int, options []string, disabled bool) picked {
	cur := ""
	if *chosen >= 0 && *chosen < len(options) {
		cur = options[*chosen]
	}
	b, changed := kit.Select(c.Key(key), "setting-"+title, title, &cur, options, disabled)
	if changed {
		for i, o := range options {
			if o == cur {
				*chosen = i
			}
		}
	}
	return picked{b, changed}
}

// picked is a Select's trigger and whether the user chose anew.
type picked struct {
	ui.Element
	changed bool
}

func (p picked) Width(w float32) picked { p.Element.Width(w).MinWidth(0); return p }
func (p picked) Fill() picked           { p.Element.FillWidth().MinWidth(0); return p }
func (p picked) Changed() bool          { return p.changed }
