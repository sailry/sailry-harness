package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) reportKeys(c *ui.Context, err error, failed, conflict, unknown string) {
	r.reportKeysRetry(c, err, failed, conflict, unknown, nil)
}

// reportKeysRetry is reportKeys whose failure and uncertain toasts offer
// Retry, which runs again.
func (r *Runtime) reportKeysRetry(c *ui.Context, err error, failed, conflict, unknown string, again func()) {
	if err == nil {
		return
	}
	outcome, key := kit.Failed, failed
	switch faultCode(err) {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		outcome, key = kit.Conflict, conflict
	case protocol.ErrorCodeOutcomeUnknown:
		outcome, key = kit.Uncertain, unknown
	}
	kit.Recover(c, outcome, i18n.T(c, key), again)
}

func (r *Runtime) providerCommand(command protocol.Command, done func(protocol.Output, bool)) {
	r.execute(command, func(c *ui.Context, out protocol.Output, err error) {
		r.reportKeys(c, err, "provider_save_failed", "provider_revision_changed", "provider_save_unknown")
		if done != nil {
			done(out, err == nil)
		}
	})
}
