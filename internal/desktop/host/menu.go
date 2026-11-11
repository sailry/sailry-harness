package host

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// OpenPort opens a forward's local address, as the page's port buttons do:
// the rail's host menu lists the forwards too.
func OpenPort(c *ui.Context, port Port, act Actions) { openPort(c, port, act) }

// OpenPorts opens the page's port picker.
func (s *State) OpenPorts() { s.ports = portPicker{open: true} }

// AskRevoke opens the confirmation that removes the host's pairing. The page
// builds it only for a host that can be revoked (Actions.Revoke).
func (s *State) AskRevoke() { s.revoking = true }

// revokeDialog confirms Remove pairing: it names the host and what follows
// once, and runs Actions.Revoke on its action.
func revokeDialog(c *ui.Context, s *State, h *Host, act Actions) {
	if act.Revoke == nil || h.Revoking {
		s.revoking = false
	}
	if !s.revoking {
		return
	}
	title := i18n.T(c, "live_revoke")
	if kit.AlertDialog(c.Key("host-revoke"), &s.revoking, title, i18n.T(c, "live_revoke_confirm", "name", h.label(c, h.Name)),
		i18n.T(c, "settings_cancel"), title) == 1 {
		act.Revoke()
	}
}
