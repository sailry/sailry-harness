package logging

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRemoteSSHFields(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name, area, operation, phase string
		code                         protocol.ErrorCode
		stage, reason                any
		allowed                      bool
	}{
		{"check", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, "authenticate", string(protocol.SSHAuthFailed), true},
		{"install exit", "command", "install_host", Failed, protocol.ErrorCodeUnavailable, "read_result", string(protocol.SSHCommandNonzeroExit), true},
		{"install interrupted", "command", "install_host", Failed, protocol.ErrorCodeOutcomeUnknown, "wait_command", string(protocol.SSHCommandInterrupted), true},
		{"unknown reason", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, "authenticate", "private_token", false},
		{"mismatched stage", "command", "install_host", Failed, protocol.ErrorCodeUnavailable, "dial", string(protocol.SSHCommandNonzeroExit), false},
		{"empty stage and reason", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, "", "", false},
		{"missing stage", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, nil, string(protocol.SSHAuthFailed), false},
		{"non-string reason", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, "authenticate", true, false},
		{"non-string stage", "command", "check_ssh", Failed, protocol.ErrorCodeUnavailable, true, string(protocol.SSHAuthFailed), false},
		{"wrong area", "desktop", "check_ssh", Failed, protocol.ErrorCodeUnavailable, "authenticate", string(protocol.SSHAuthFailed), false},
		{"wrong operation", "command", "update_ssh_host", Failed, protocol.ErrorCodeUnavailable, "read_result", string(protocol.SSHCommandNonzeroExit), false},
		{"completed", "command", "check_ssh", Completed, protocol.ErrorCodeUnavailable, "authenticate", string(protocol.SSHAuthFailed), false},
		{"interrupted", "command", "install_host", Interrupted, protocol.ErrorCodeOutcomeUnknown, "wait_command", string(protocol.SSHCommandInterrupted), false},
		{"cancelled phase", "command", "check_ssh", Cancelled, protocol.ErrorCodeUnavailable, "authenticate", string(protocol.SSHAuthFailed), false},
		{"cancelled code", "command", "check_ssh", Failed, protocol.ErrorCodeCancelled, "authenticate", string(protocol.SSHAuthFailed), false},
	} {
		t.Run(test.name, func(t *testing.T) {
			got := Remote(Fields{
				"area": test.area, "operation": test.operation, "phase": test.phase, "code": string(test.code),
				"stage": test.stage, "reason": test.reason, "command_stage": "completion",
			})
			stage, hasStage := got["stage"]
			reason, hasReason := got["reason"]
			if hasStage != test.allowed || hasReason != test.allowed || test.allowed && (stage != test.stage || reason != test.reason) {
				t.Errorf("unexpected SSH fields: %v", got)
			}
			if got["command_stage"] != "completion" {
				t.Errorf("command stage lost: %v", got)
			}
		})
	}
}
