package wsl

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	discoveryLimit    = 64 * 1024
	discoveryDeadline = 5 * time.Second
	commandLimit      = 128 * 1024
	// cleanup bounds how long a stopped launcher may keep its pipes open.
	cleanup = 2 * time.Second
)

var (
	errDeadline = errors.New("WSL launcher deadline elapsed")
	errOverflow = errors.New("WSL launcher output exceeds its limit")
)

// capture retains at most limit bytes and stops the launcher on the first
// byte beyond it.
// The buffer is a field, not embedded: its ReadFrom would bypass the limit.
type capture struct {
	buffer   bytes.Buffer
	limit    int
	overflow func()
}

func (output *capture) Write(body []byte) (int, error) {
	if output.buffer.Len()+len(body) > output.limit {
		output.overflow()
		return 0, errOverflow
	}
	return output.buffer.Write(body)
}

type completion struct {
	launched       bool
	stdout, stderr []byte
	// err is the launch or wait error; cause is why this package stopped
	// the launcher, when it did.
	err, cause error
}

// execute owns one launcher process tree until it exits. A nil script
// connects no input and discards diagnostics; otherwise the script is the
// whole standard input and diagnostics are captured.
func execute(ctx context.Context, deadline time.Duration, executable string, arguments, environment []string, script *string, limit int) completion {
	operation, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	timer := time.AfterFunc(deadline, func() { cancel(errDeadline) })
	defer timer.Stop()
	overflow := func() { cancel(errOverflow) }
	command := exec.CommandContext(operation, executable, arguments...)
	command.Env = append([]string{}, environment...)
	command.Dir = filepath.Dir(executable)
	command.WaitDelay = cleanup
	stdout := &capture{limit: limit, overflow: overflow}
	stderr := &capture{limit: limit, overflow: overflow}
	command.Stdout = stdout
	if script != nil {
		command.Stdin = strings.NewReader(*script)
		command.Stderr = stderr
	}
	release, err := start(command)
	if err != nil {
		return completion{err: err}
	}
	err = command.Wait()
	release()
	return completion{launched: true, stdout: stdout.buffer.Bytes(), stderr: stderr.buffer.Bytes(), err: err, cause: context.Cause(operation)}
}

// query runs a discovery launcher and returns its bounded raw output.
func query(ctx context.Context, executable string, arguments, environment []string, deadline time.Duration) ([]byte, error) {
	cancelled := &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "WSL discovery was cancelled"}
	if ctx.Err() != nil {
		return nil, cancelled
	}
	done := execute(ctx, deadline, executable, arguments, environment, nil, discoveryLimit)
	switch {
	case !done.launched:
		return nil, discoveryFailure("launch failed: " + done.err.Error())
	case errors.Is(done.err, exec.ErrWaitDelay):
		return nil, discoveryFailure("process could not be stopped")
	case ctx.Err() != nil:
		return nil, cancelled
	case done.cause == errDeadline:
		return nil, discoveryFailure("timed out")
	case done.cause == errOverflow:
		return nil, discoveryFailure("output exceeds the supported size")
	case done.err == nil:
		return done.stdout, nil
	}
	var exit *exec.ExitError
	if !errors.As(done.err, &exit) {
		return nil, discoveryFailure("wait failed: " + done.err.Error())
	}
	code := exit.ExitCode()
	if code == -1 {
		return nil, discoveryFailure("terminated without an exit code")
	}
	return nil, discoveryFailure(fmt.Sprintf("failed with exit code %d (0x%08X)", int32(code), uint32(code)))
}

// run feeds one script to a launcher and returns its bounded UTF-8 output.
// Any failure after the script may have started reports an unknown outcome.
func run(ctx context.Context, executable string, arguments, environment []string, script string, deadline time.Duration) (string, error) {
	cancelled := &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "WSL Host installation was cancelled"}
	if ctx.Err() != nil {
		return "", cancelled
	}
	done := execute(ctx, deadline, executable, arguments, environment, &script, commandLimit)
	switch {
	case !done.launched:
		return "", unavailable("WSL launch failed: " + done.err.Error())
	case errors.Is(done.err, exec.ErrWaitDelay):
		return "", unknown("WSL command could not be stopped; inspect the distribution before retrying")
	case ctx.Err() != nil:
		return "", cancelled
	case done.cause == errDeadline:
		return "", unknown("WSL command timed out; inspect the distribution before retrying")
	case done.cause == errOverflow:
		return "", unknown("WSL command output exceeds the supported size")
	case done.err == nil:
		if !utf8.Valid(done.stdout) {
			return "", unknown("WSL command output is not UTF-8")
		}
		return string(done.stdout), nil
	}
	var exit *exec.ExitError
	if !errors.As(done.err, &exit) {
		return "", unknown("WSL command status is unavailable")
	}
	// Exit code 73 is the installer's own report that it ran without
	// confirming completion; a signal or negative status is equally unknown.
	code := exit.ExitCode()
	if code == -1 || int32(code) == 73 || int32(code) < 0 {
		return "", unknown("Host setup is unconfirmed; inspect the distribution before retrying")
	}
	return "", unavailable(fmt.Sprintf("WSL command failed (%d): %s", code, strings.TrimSpace(strings.ToValidUTF8(string(done.stderr), "\uFFFD"))))
}

func discoveryFailure(message string) error {
	return unavailable("WSL discovery " + message)
}
