//go:build !linux

package process

import "github.com/sailry/sailry-harness/internal/protocol"

// SandboxTerminal refuses a workspace terminal: a macOS sandbox profile must
// name the pseudo-terminal device, which the terminal runtime opens inside its
// own launch and never reports. Windows has no workspace sandbox and never
// asks.
func (s *Service) SandboxTerminal(string, []string, []string) ([]string, []string, func(), error) {
	return nil, nil, nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "workspace terminals are unavailable on this platform; open the terminal with full access"}
}
