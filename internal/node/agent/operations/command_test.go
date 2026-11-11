package operations

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandAccessFollowsSandboxSetting(t *testing.T) {
	t.Parallel()
	for name, test := range map[string]struct {
		arguments string
		sandbox   bool
		want      protocol.ProcessAccess
	}{
		"sandbox on defaults to workspace":          {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[]}`, true, protocol.ProcessAccessWorkspace},
		"sandbox on keeps a null as the default":    {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[],"access":null}`, true, protocol.ProcessAccessWorkspace},
		"sandbox on keeps requested unrestricted":   {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[],"access":"unrestricted"}`, true, protocol.ProcessAccessUnrestricted},
		"sandbox on keeps requested workspace":      {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[],"access":"workspace"}`, true, protocol.ProcessAccessWorkspace},
		"sandbox off defaults to unrestricted":      {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[]}`, false, protocol.ProcessAccessUnrestricted},
		"sandbox off overrides requested workspace": {`{"command":"ls","description":null,"cwd":"","timeout_ms":120000,"background":false,"attachments":[],"access":"workspace"}`, false, protocol.ProcessAccessUnrestricted},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			command, err := Command(protocol.ToolOperationRunCommand, json.RawMessage(test.arguments), protocol.NewWorktreeID(), protocol.NewSessionID(), protocol.NewTurnID(), nil, test.sandbox)
			if err != nil {
				t.Fatal(err)
			}
			input, ok := command.Data.(*protocol.CommandRunCommandPayload)
			if !ok || command.Kind != "run_command" {
				t.Fatalf("unexpected command: %+v", command)
			}
			if input.Access != test.want {
				t.Fatalf("access = %q, want %q", input.Access, test.want)
			}
		})
	}
}
