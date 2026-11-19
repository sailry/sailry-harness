package operations

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandProcessToolArguments(t *testing.T) {
	t.Parallel()
	worktree, session, turn := protocol.NewWorktreeID(), protocol.NewSessionID(), protocol.NewTurnID()
	command, err := Command(protocol.ToolOperationRunCommand, json.RawMessage(`{"command":"pwd"}`), worktree, session, turn, nil, true)
	if err != nil {
		t.Fatal(err)
	}
	launch := command.Data.(*protocol.CommandRunCommandPayload)
	if launch.Turn != turn || launch.TimeoutMs != 120000 || launch.Background || launch.Pty || launch.FollowUp || launch.Cwd != "" || launch.Description != nil || len(launch.Attachments) != 0 {
		t.Fatalf("unexpected launch defaults: %+v", launch)
	}
	id := protocol.NewRequestID()
	for _, arguments := range []struct {
		operation protocol.ToolOperation
		value     string
	}{
		{protocol.ToolOperationRunCommand, `{"command":"pwd","pty":true}`},
		{protocol.ToolOperationRunCommand, `{"command":"pwd","description":" "}`},
		{protocol.ToolOperationReadCommand, `{"wait_ms":100}`},
		{protocol.ToolOperationReadCommand, `{"offset":12}`},
		{protocol.ToolOperationWriteCommand, `{"id":"` + id.String() + `","input":"` + strings.Repeat("\u00e9", 32769) + `"}`},
	} {
		if _, err := Command(arguments.operation, json.RawMessage(arguments.value), worktree, session, turn, nil, true); err == nil {
			t.Errorf("%s accepted an invalid command", arguments.operation)
		}
	}
	revision := strings.Repeat("a", 64)
	command, err = Command(protocol.ToolOperationReadCommand, json.RawMessage(`{"output":{"call":"command-result","revision":"`+revision+`"},"stream":"stderr"}`), worktree, session, turn, nil, true)
	if err != nil {
		t.Fatal(err)
	}
	page := command.Data.(*protocol.CommandReadToolOutputPayload)
	if page.Turn != turn || page.Output.Call != "command-result" || page.Output.Revision != revision || page.Stream == nil || *page.Stream != protocol.ProcessOutputOutputStreamStderr || page.Offset != 0 || page.Limit != 32768 {
		t.Fatalf("unexpected retained output read: %+v", page)
	}
}

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
