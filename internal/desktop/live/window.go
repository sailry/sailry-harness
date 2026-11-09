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
