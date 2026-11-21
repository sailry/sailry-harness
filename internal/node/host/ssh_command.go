package host

import (
	"bytes"
	"context"
	"errors"
	"io"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

const sshOutputLimit = 128 * 1024

type sshExecution uint8

const (
	sshProbe sshExecution = iota
	sshStaging
	sshInstaller
)

func installFault(code protocol.ErrorCode, message string, reason protocol.SSHFailureReason) *protocol.Fault {
	return &protocol.Fault{Code: code, Message: message, SSHReason: reason}
}

func (execution sshExecution) unconfirmed(message string, reason protocol.SSHFailureReason) *protocol.Fault {
	code := protocol.ErrorCodeOutcomeUnknown
	if execution == sshProbe {
		code = protocol.ErrorCodeUnavailable
	}
	return installFault(code, message, reason)
}

type limitedOutput struct {
	buffer    bytes.Buffer
	truncated bool
}

func (output *limitedOutput) Write(body []byte) (int, error) {
	count := min(len(body), sshOutputLimit-output.buffer.Len())
	_, _ = output.buffer.Write(body[:count])
	output.truncated = output.truncated || count < len(body)
	return len(body), nil
}

func runSSH(ctx context.Context, connection *cryptossh.Client, execution sshExecution, command string, input io.Reader) (protocol.SshOutcomeCompletedPayload, error) {
	stopConnection := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stopConnection()
	session, err := connection.NewSession()
	if err != nil {
		return protocol.SshOutcomeCompletedPayload{}, installFault(protocol.ErrorCodeUnavailable, "SSH channel could not be opened", protocol.SSHChannelUnavailable)
	}
	defer session.Close()
	var stdout, stderr limitedOutput
	session.Stdout, session.Stderr, session.Stdin = &stdout, &stderr, input
	if ctx.Err() != nil {
		return protocol.SshOutcomeCompletedPayload{}, installFault(protocol.ErrorCodeUnavailable, "SSH command was not sent", protocol.SSHCommandNotSent)
	}
	if err := session.Start(command); err != nil {
		// Session.Start uses this exact error for a negative exec reply.
		if err.Error() == "ssh: command "+command+" failed" {
			return protocol.SshOutcomeCompletedPayload{}, installFault(protocol.ErrorCodeUnavailable, "SSH server refused to run the command", protocol.SSHCommandRejected)
		}
		return protocol.SshOutcomeCompletedPayload{}, execution.unconfirmed("SSH command receipt is unavailable", protocol.SSHCommandReceiptLost)
	}
	err = session.Wait()
	result := protocol.SshOutcomeCompletedPayload{
		Stdout:    strings.ToValidUTF8(stdout.buffer.String(), "\uFFFD"),
		Stderr:    strings.ToValidUTF8(stderr.buffer.String(), "\uFFFD"),
		Truncated: stdout.truncated || stderr.truncated,
	}
	if err == nil {
		return result, nil
	}
	var exit *cryptossh.ExitError
	if errors.As(err, &exit) && exit.ExitStatus() >= 0 {
		if exit.Signal() != "" {
			return result, execution.unconfirmed("SSH command was interrupted", protocol.SSHCommandInterrupted)
		}
		result.ExitCode = uint32(exit.ExitStatus())
		return result, nil
	}
	var missing *cryptossh.ExitMissingError
	switch {
	case ctx.Err() != nil:
		return result, execution.unconfirmed("SSH command was interrupted", protocol.SSHCommandInterrupted)
	case errors.As(err, &missing):
		return result, execution.unconfirmed("SSH command ended without an exit status", protocol.SSHCommandExitMissing)
	default:
		return result, execution.unconfirmed("SSH command result is unavailable", protocol.SSHCommandResultUnavailable)
	}
}

func commandSSH(ctx context.Context, connection *cryptossh.Client, execution sshExecution, command string, input io.Reader) (string, error) {
	result, err := runSSH(ctx, connection, execution, command, input)
	if err != nil {
		return "", err
	}
	if execution == sshInstaller && result.ExitCode == 73 {
		return "", execution.unconfirmed("Host installer ran but completion could not be confirmed; inspect the Host before retrying", protocol.SSHInstallRollbackUnconfirmed)
	}
	if result.ExitCode != 0 {
		message := "Host installation command failed"
		if command == stageCommand && result.ExitCode == 1 && !result.Truncated && strings.TrimSpace(result.Stderr) == stagePythonRequired {
			message += ": " + stagePythonRequired
		}
		return "", installFault(protocol.ErrorCodeUnavailable, message, protocol.SSHCommandNonzeroExit)
	}
	if result.Truncated {
		return "", execution.unconfirmed("Host installation command output exceeded its limit", protocol.SSHCommandOutputTruncated)
	}
	return result.Stdout, nil
}

func shellQuote(value string) (string, error) {
	if strings.ContainsRune(value, '\x00') {
		return "", invalid("shell argument contains a null byte")
	}
	return "'" + strings.ReplaceAll(value, "'", "'\"'\"'") + "'", nil
}
