package store

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPlanRunCommandRequiresReadOnlySandbox(t *testing.T) {
	t.Parallel()
	input := protocol.DefaultCommandRunCommandPayload()
	input.Command, input.Access = "git status", protocol.ProcessAccessWorkspace
	for _, test := range []struct {
		name               string
		supported, enabled bool
		change             func(*protocol.CommandRunCommandPayload)
	}{
		{name: "disabled sandbox", supported: true},
		{name: "unsupported OS", enabled: true},
		{name: "unrestricted access", supported: true, enabled: true, change: func(v *protocol.CommandRunCommandPayload) { v.Access = protocol.ProcessAccessUnrestricted }},
		{name: "background", supported: true, enabled: true, change: func(v *protocol.CommandRunCommandPayload) { v.Background = true }},
		{name: "pty", supported: true, enabled: true, change: func(v *protocol.CommandRunCommandPayload) { v.Pty = true }},
		{name: "service", supported: true, enabled: true, change: func(v *protocol.CommandRunCommandPayload) { v.Service = true }},
	} {
		t.Run(test.name, func(t *testing.T) {
			command := input
			if test.change != nil {
				test.change(&command)
			}
			if err := CheckPlanRunCommand(command, test.supported, test.enabled); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				t.Fatalf("Plan command was not refused: %v", err)
			}
		})
	}
	// The sandbox, not a command allowlist, establishes authority.
	input.Command = "printf '%s\\n' \"$(pwd)\""
	if err := CheckPlanRunCommand(input, true, true); err != nil {
		t.Fatal(err)
	}
}
