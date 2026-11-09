package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// ReportWindowFailure reports composition failure in the originating window.
func (r *Runtime) ReportWindowFailure() {
	r.enqueue(func(c *ui.Context) {
		kit.Toast(c, kit.Failed, i18n.T(c, "session_window_failed"))
	})
}

func (r *Runtime) ReportReaderFailure() {
	r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "reader_failed")) })
}
func (r *Runtime) ReportExport(err error) {
	r.enqueue(func(c *ui.Context) {
		if err != nil {
			kit.Toast(c, kit.Failed, i18n.T(c, "conversation_export_failed"))
		} else {
			kit.Toast(c, kit.Done, i18n.T(c, "conversation_exported"))
		}
	})
}
