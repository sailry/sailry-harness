package protocol

import "encoding/json"

type SSHFailureReason string

const (
	SSHCredentialInvalid          SSHFailureReason = "ssh_credential_invalid"
	SSHKeyFileUnavailable         SSHFailureReason = "ssh_key_file_unavailable"
	SSHAgentUnavailable           SSHFailureReason = "ssh_agent_unavailable"
	SSHDialFailed                 SSHFailureReason = "ssh_dial_failed"
	SSHDialTimeout                SSHFailureReason = "ssh_dial_timeout"
	SSHDialCancelled              SSHFailureReason = "ssh_dial_cancelled"
	SSHHandshakeFailed            SSHFailureReason = "ssh_handshake_failed"
	SSHHandshakeTimeout           SSHFailureReason = "ssh_handshake_timeout"
	SSHHandshakeCancelled         SSHFailureReason = "ssh_handshake_cancelled"
	SSHAuthFailed                 SSHFailureReason = "ssh_auth_failed"
	SSHAuthTimeout                SSHFailureReason = "ssh_auth_timeout"
	SSHAuthCancelled              SSHFailureReason = "ssh_auth_cancelled"
	SSHChannelUnavailable         SSHFailureReason = "ssh_channel_unavailable"
	SSHCommandNotSent             SSHFailureReason = "ssh_command_not_sent"
	SSHCommandRejected            SSHFailureReason = "ssh_command_rejected"
	SSHCommandReceiptLost         SSHFailureReason = "ssh_command_receipt_lost"
	SSHCommandInterrupted         SSHFailureReason = "ssh_command_interrupted"
	SSHCommandExitMissing         SSHFailureReason = "ssh_command_exit_missing"
	SSHCommandResultUnavailable   SSHFailureReason = "ssh_command_result_unavailable"
	SSHCommandNonzeroExit         SSHFailureReason = "ssh_command_nonzero_exit"
	SSHCommandOutputTruncated     SSHFailureReason = "ssh_command_output_truncated"
	SSHInstallRollbackUnconfirmed SSHFailureReason = "ssh_install_rollback_unconfirmed"
	SSHBootstrapTicketUnavailable SSHFailureReason = "ssh_bootstrap_ticket_unavailable"
	SSHPairingUnconfirmed         SSHFailureReason = "ssh_pairing_unconfirmed"
	SSHUpdateUnconfirmed          SSHFailureReason = "ssh_update_unconfirmed"
)

func (reason SSHFailureReason) Stage() string {
	switch reason {
	case SSHCredentialInvalid, SSHKeyFileUnavailable, SSHAgentUnavailable:
		return "prepare_auth"
	case SSHDialFailed, SSHDialTimeout, SSHDialCancelled:
		return "dial"
	case SSHHandshakeFailed, SSHHandshakeTimeout, SSHHandshakeCancelled:
		return "handshake"
	case SSHAuthFailed, SSHAuthTimeout, SSHAuthCancelled:
		return "authenticate"
	case SSHChannelUnavailable:
		return "open_channel"
	case SSHCommandNotSent, SSHCommandRejected, SSHCommandReceiptLost:
		return "start_command"
	case SSHCommandInterrupted, SSHCommandExitMissing, SSHCommandResultUnavailable:
		return "wait_command"
	case SSHCommandNonzeroExit, SSHCommandOutputTruncated, SSHInstallRollbackUnconfirmed:
		return "read_result"
	case SSHBootstrapTicketUnavailable:
		return "read_bootstrap_ticket"
	case SSHPairingUnconfirmed:
		return "pair_host"
	case SSHUpdateUnconfirmed:
		return "confirm_update"
	}
	return ""
}

func (reason SSHFailureReason) MarshalJSON() ([]byte, error) { return marshalEnum(reason) }

func (reason *SSHFailureReason) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	if SSHFailureReason(decoded).Stage() == "" {
		return unknownEnum(data, reason, decoded)
	}
	*reason = SSHFailureReason(decoded)
	return nil
}
