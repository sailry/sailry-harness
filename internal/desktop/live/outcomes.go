package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) reportKeys(c *ui.Context, err error, failed, conflict, unknown string) {
	if err == nil {
		return
	}
	outcome, key := outcomeKey(err, failed, conflict, unknown)
	kit.Toast(c, outcome, i18n.T(c, key))
	// The failed key names the action whichever outcome was shown.
	kit.RecordFailure("desktop", failed, outcome, err)
}

// outcomeKey selects the toast for a failed action by its fault.
func outcomeKey(err error, failed, conflict, unknown string) (kit.Outcome, string) {
	outcome, key := kit.Failed, failed
	switch faultCode(err) {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		outcome, key = kit.Conflict, conflict
	case protocol.ErrorCodeOutcomeUnknown:
		outcome, key = kit.Uncertain, unknown
	}
	if faultMessage(err) == "resource host is offline" {
		key = "resource_offline_action"
	}
	return outcome, key
}

func (r *Runtime) providerCommand(command protocol.Command, done func(protocol.Output, bool)) {
	r.execute(command, func(c *ui.Context, out protocol.Output, err error) {
		r.reportKeys(c, err, "provider_save_failed", "provider_revision_changed", "provider_save_unknown")
		if done != nil {
			done(out, err == nil)
		}
	})
}
