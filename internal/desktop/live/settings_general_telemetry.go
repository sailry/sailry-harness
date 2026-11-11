package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › General › Diagnostics: the identity reset and the diagnostics
// report. Both share the switches' pending state, so one telemetry request
// runs at a time.

// telemetryOutcome is the toast of a reset or a diagnostics report.
func telemetryOutcome(err error, diagnostics bool) (kit.Outcome, string) {
	switch code := faultCode(err); {
	case err == nil && diagnostics:
		return kit.Done, "telemetry_diagnostics_sent"
	case err == nil:
		return kit.Done, "telemetry_reset"
	case diagnostics:
		// Nothing durable was written: the report was sent or it was not.
		return kit.Failed, "telemetry_diagnostics_failed"
	case code == protocol.ErrorCodeRevisionConflict || code == protocol.ErrorCodeConflict:
		return kit.Conflict, "telemetry_conflict"
	case code == protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain, "telemetry_unknown"
	}
	return kit.Failed, "telemetry_failed"
}

func (r *Runtime) resetTelemetry() {
	m := &r.App.Settings.Telemetry
	if m.Pending {
		return
	}
	m.Pending = true
	r.App.Settings.SetBusy(settings.BusyTelemetry, true)
	command := protocol.Command{Kind: "reset_telemetry_identity", Data: protocol.CommandResetTelemetryIdentityPayload{ExpectedRevision: m.Revision}}
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		m.Pending = false
		r.App.Settings.SetBusy(settings.BusyTelemetry, false)
		outcome, key := telemetryOutcome(err, false)
		kit.Toast(c, outcome, i18n.T(c, key))
		if value, valid := detail[protocol.TelemetrySettings](output.Data); err == nil && valid && output.Kind == "telemetry_settings" {
			m.Revision = value.Revision
		}
	})
}

func (r *Runtime) sendDiagnostics() {
	m := &r.App.Settings.Telemetry
	if m.Pending {
		return
	}
	m.Pending = true
	r.App.Settings.SetBusy(settings.BusyDiagnostics, true)
	r.execute(protocol.Command{Kind: "send_diagnostics"}, func(c *ui.Context, output protocol.Output, err error) {
		m.Pending = false
		r.App.Settings.SetBusy(settings.BusyDiagnostics, false)
		if err == nil && output.Kind != "diagnostics_sent" {
			err = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected diagnostics response"}
		}
		outcome, key := telemetryOutcome(err, true)
		kit.Toast(c, outcome, i18n.T(c, key))
	})
}
