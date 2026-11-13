package acceptance

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandInputApprovalIsLimitedToOwnedFixture(t *testing.T) {
	t.Parallel()
	id := protocol.RequestID{1}
	for _, test := range []struct {
		name, tool, target, input string
		valid                     bool
	}{
		{"input", "write_command", id.String(), "SAILRY-INPUT\n", true},
		{"stop", "stop_command", id.String(), "", true},
		{"another_process", "write_command", (protocol.RequestID{2}).String(), "SAILRY-INPUT\n", false},
		{"different_input", "write_command", id.String(), "another command\n", false},
		{"missing_newline", "write_command", id.String(), "SAILRY-INPUT", false},
		{"another_tool", "run_command", id.String(), "SAILRY-INPUT\n", false},
		{"missing_id", "stop_command", "", "", false},
	} {
		t.Run(test.name, func(t *testing.T) {
			fields := map[string]string{"id": test.target}
			if test.tool == "write_command" {
				fields["input"] = test.input
			}
			arguments, err := json.Marshal(fields)
			if err != nil {
				t.Fatal(err)
			}
			detail := protocol.DefaultToolDetail()
			detail.Arguments = arguments
			if got := commandInputApprovalMatches(test.tool, detail, id); got != test.valid {
				t.Fatalf("fixture approval = %v, want %v", got, test.valid)
			}
			if commandInputApprovalMatches(test.tool, detail, protocol.RequestID{}) {
				t.Fatal("approval accepted without an owned process")
			}
		})
	}
}

func TestForegroundOutputUsesItsRetainedIdentity(t *testing.T) {
	t.Parallel()
	id := protocol.RequestID{1}
	for _, test := range []struct {
		name, result string
		valid        bool
	}{
		{"foreground", `{"kind":"command_result","sailry_command":{"id":"` + id.String() + `","stdout":{"total_bytes":108894}}}`, true},
		{"background", `{"kind":"command_started","data":{"id":"` + id.String() + `"}}`, false},
		{"missing", `{"kind":"command_result"}`, false},
		{"zero", `{"kind":"command_result","sailry_command":{"id":"` + (protocol.RequestID{}).String() + `"}}`, false},
		{"invalid", `{"kind":"command_result","sailry_command":{"id":"PID-123"}}`, false},
		{"malformed", `{`, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			result := json.RawMessage(test.result)
			detail := protocol.DefaultToolDetail()
			detail.Result = &result
			got, err := retainedCommandID(detail)
			if (err == nil) != test.valid || test.valid && got != id {
				t.Fatalf("retained identity = %v, %v", got, err)
			}
		})
	}
	if _, err := retainedCommandID(protocol.DefaultToolDetail()); err == nil {
		t.Fatal("a missing raw result had a retained identity")
	}
}
