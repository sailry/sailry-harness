package settings

import "github.com/sailry/sailry-harness/internal/desktop/preview"

// ConnStatus is a service's status: the i18n key of a problem, or "" with the
// status kind. Reason is why a failed or unavailable service is so, when the
// host said.
type ConnStatus struct {
	Problem    string
	Connecting bool
	Failed     bool
	Reason     string
	Tools      int // connected: its tools; -1 not connected
}

// StatusOf derives a preview service's connection state.
func StatusOf(s *preview.Service) ConnStatus {
	if s.Account {
		if !s.AccountOn {
			return ConnStatus{Tools: -1}
		}
		switch s.State {
		case preview.Unconnected:
			return ConnStatus{Problem: "connector_not_connected", Tools: -1}
		case preview.Expired:
			return ConnStatus{Problem: "connector_expired", Tools: -1}
		case preview.NeedsConfirmation:
			return ConnStatus{Problem: "connector_confirmation", Tools: -1}
		case preview.Connecting:
			return ConnStatus{Connecting: true, Tools: -1}
		case preview.ConnFailed:
			return ConnStatus{Problem: "connector_failed", Tools: -1}
		}
	}
	tools, up, connecting := 0, false, false
	for _, x := range s.Servers {
		if !x.Allowed {
			return ConnStatus{Problem: "connector_server_disabled", Tools: -1}
		}
		if x.Problem != "" {
			return ConnStatus{Problem: x.Problem, Reason: x.Error, Tools: -1}
		}
		switch x.State {
		case preview.ServerAuth:
			return ConnStatus{Problem: "connector_not_connected", Tools: -1}
		case preview.ServerDown:
			return ConnStatus{Failed: true, Reason: x.Error, Tools: -1}
		case preview.ServerStarting:
			connecting = true
		case preview.ServerUp:
			up = true
			tools += x.Tools
		}
	}
	switch {
	case connecting:
		return ConnStatus{Connecting: true, Tools: -1}
	case up:
		return ConnStatus{Tools: tools}
	case !s.Account:
		// Nothing connected yet: no server, or none asked to connect.
		return ConnStatus{Problem: "connector_not_connected", Tools: -1}
	}
	return ConnStatus{Tools: -1}
}
