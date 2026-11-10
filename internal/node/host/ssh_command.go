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

type limitedOutput struct {
	bytes.Buffer
	truncated bool
}

func (output *limitedOutput) Write(body []byte) (int, error) {
	count := min(len(body), sshOutputLimit-output.Len())
	_, _ = output.Buffer.Write(body[:count])
	output.truncated = output.truncated || count < len(body)
	return len(body), nil
}

func runSSH(ctx context.Context, connection *cryptossh.Client, command string, input io.Reader) (protocol.SshOutcomeCompletedPayload, error) {
	stopConnection := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stopConnection()
	session, err := connection.NewSession()
	if err != nil {
		return protocol.SshOutcomeCompletedPayload{}, unavailable("SSH channel could not be opened")
	}
	defer session.Close()
	stop := context.AfterFunc(ctx, func() { _ = session.Close() })
	defer stop()
	var stdout, stderr limitedOutput
	session.Stdout, session.Stderr, session.Stdin = &stdout, &stderr, input
	if err := session.Start(command); err != nil {
		return protocol.SshOutcomeCompletedPayload{}, unknown("SSH command receipt is unavailable")
	}
	err = session.Wait()
	if ctx.Err() != nil {
		return protocol.SshOutcomeCompletedPayload{}, unknown("SSH command interrupted; inspect the remote target before retrying")
	}
	result := protocol.SshOutcomeCompletedPayload{Stdout: strings.ToValidUTF8(stdout.String(), "\uFFFD"), Stderr: strings.ToValidUTF8(stderr.String(), "\uFFFD"), Truncated: stdout.truncated || stderr.truncated}
	if err != nil {
		var exit *cryptossh.ExitError
		if !errors.As(err, &exit) || exit.ExitStatus() < 0 {
			return result, unknown("SSH command ended without an exit status")
		}
		result.ExitCode = uint32(exit.ExitStatus())
	}
	return result, nil
}

func commandSSH(ctx context.Context, connection *cryptossh.Client, command string, input io.Reader) (string, error) {
	result, err := runSSH(ctx, connection, command, input)
	if err != nil {
		return "", err
	}
	if result.ExitCode == 73 {
		return "", unknown("Host installer ran but completion could not be confirmed; inspect the Host before retrying")
	}
	if result.ExitCode != 0 {
		return "", unavailable("Host installation command failed: " + strings.TrimSpace(result.Stderr))
	}
	if result.Truncated {
		return "", unknown("Host installation command output exceeded its limit")
	}
	return result.Stdout, nil
}

func shellQuote(value string) (string, error) {
	if strings.ContainsRune(value, '\x00') {
		return "", invalid("shell argument contains a null byte")
	}
	return "'" + strings.ReplaceAll(value, "'", "'\"'\"'") + "'", nil
}
