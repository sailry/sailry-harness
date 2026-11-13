package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"runtime"
	"time"

	core "github.com/sailry/sailry-harness/internal/acceptance"
)

func runAcceptanceChild(ctx context.Context, executable, directory, socket string, options core.Options, emit func(core.CaseResult)) error {
	if executable == "" {
		return errors.New("UI acceptance requires --desktop with the headless Desktop executable")
	}
	args := []string{"--acceptance-ui", "--acceptance-dir", directory, "--acceptance-mode", options.Suite}
	if socket != "" {
		args = append(args, "--acceptance-socket", socket)
	}
	if options.Suite == "replay" {
		args = append(args, "--acceptance-replay", options.ReportDirectory)
	}
	command := exec.CommandContext(ctx, executable, args...)
	// An interrupt lets RunLive stop its own turns before the child exits.
	// WaitDelay bounds a broken child's shutdown, after that cleanup window.
	command.Cancel = func() error {
		if runtime.GOOS == "windows" {
			return command.Process.Kill()
		}
		return command.Process.Signal(os.Interrupt)
	}
	command.WaitDelay = 30 * time.Second
	input, err := json.Marshal(options)
	if err != nil {
		return err
	}
	command.Stdin = bytes.NewReader(input)
	command.Stderr = io.Discard
	output, err := command.StdoutPipe()
	if err != nil {
		return err
	}
	if err := command.Start(); err != nil {
		return err
	}
	decoder := json.NewDecoder(output)
	var reported bool
	var reportOK bool
	var decodeErr error
	for {
		var event struct {
			Kind   string           `json:"kind"`
			Case   *core.CaseResult `json:"case,omitempty"`
			Report *core.Report     `json:"report,omitempty"`
			Error  string           `json:"error,omitempty"`
		}
		if err := decoder.Decode(&event); err != nil {
			if !errors.Is(err, io.EOF) {
				decodeErr = errors.New("Invalid acceptance subprocess output")
			}
			break
		}
		switch event.Kind {
		case "case":
			if event.Case == nil || reported {
				decodeErr = errors.New("Invalid acceptance subprocess case")
			} else if emit != nil {
				emit(*event.Case)
			}
		case "report":
			if event.Report == nil || reported {
				decodeErr = errors.New("Invalid acceptance subprocess report")
			} else {
				reported = true
				reportOK = event.Report.OK()
				if event.Error != "" {
					decodeErr = errors.New(event.Error)
				}
			}
		default:
			decodeErr = errors.New("Unknown acceptance subprocess event")
		}
		if decodeErr != nil {
			break
		}
	}
	if decodeErr != nil {
		_ = command.Process.Kill()
	}
	waitErr := command.Wait()
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if decodeErr != nil {
		return decodeErr
	}
	if waitErr != nil {
		// Child diagnostics stay out of reports: UI runtime logs may refer to
		// unrelated data on the same Node. The test cases carry scoped evidence.
		var exit *exec.ExitError
		if !(reported && !reportOK && errors.As(waitErr, &exit) && exit.ExitCode() == 1) {
			return fmt.Errorf("Acceptance subprocess failed: %w", waitErr)
		}
	}
	if !reported {
		return errors.New("Acceptance subprocess exited without a report")
	}
	return nil
}
