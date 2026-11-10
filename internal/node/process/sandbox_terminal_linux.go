package process

import (
	"os"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SandboxTerminal wraps a workspace terminal's program in the command
// sandbox. The returned variables follow the terminal's own environment, and
// release removes the private state once the terminal has ended.
func (s *Service) SandboxTerminal(root string, command, environment []string) ([]string, []string, func(), error) {
	policy, release, err := prepareSandbox(Launch{Root: root, Profile: s.options.Profile, Access: protocol.ProcessAccessWorkspace}, s.options.Environment, s.options.CaptureManaged)
	if err != nil {
		return nil, nil, nil, err
	}
	program, args, err := sandboxCommand(policy, command[0], command[1:], "")
	if err != nil {
		release()
		return nil, nil, nil, err
	}
	variables := environmentValues(policy.variables)
	// The dynamic loader runs before the wrapper applies confinement, and a
	// terminal can only add to the inherited environment.
	for _, entry := range append(os.Environ(), environment...) {
		if key, _, _ := strings.Cut(entry, "="); strings.HasPrefix(key, "LD_") {
			variables = append(variables, key+"=")
		}
	}
	return append([]string{program}, args...), variables, release, nil
}
