package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"reflect"
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
	input, err := json.Marshal(options)
	if err != nil {
		return err
	}
	return runAcceptanceProcess(ctx, executable, args, input, emit, 30*time.Second)
}

func runAcceptanceProcess(ctx context.Context, executable string, args []string, input []byte, emit func(core.CaseResult), shutdownDelay time.Duration) error {
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	command := exec.CommandContext(scoped, executable, args...)
	configureAcceptanceProcess(command)
	command.WaitDelay = shutdownDelay
	command.Stdin = bytes.NewReader(input)
	command.Stderr = io.Discard
	// Let exec own its stdout copy loop and start Wait immediately. With
	// StdoutPipe, waiting for decoder EOF before Wait never observes an exited
	// child whose descendant still holds stdout, so WaitDelay cannot start.
	output, writer := io.Pipe()
	defer output.Close()
	defer writer.Close()
	command.Stdout = writer
	if err := command.Start(); err != nil {
		return err
	}
	waited := make(chan error, 1)
	go func() {
		err := command.Wait()
		killAcceptanceProcess(command)
		_ = writer.Close()
		waited <- err
	}()
	decoder := json.NewDecoder(output)
	var reported bool
	var reportOK bool
	results := make(map[[2]string]core.CaseResult)
	var decodeErr error
	var exitTimer *time.Timer
	defer func() {
		if exitTimer != nil {
			exitTimer.Stop()
		}
	}()
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
			} else {
				results[[2]string{event.Case.Group, event.Case.ID}] = *event.Case
				if emit != nil {
					emit(*event.Case)
				}
			}
		case "report":
			if event.Report == nil || reported {
				decodeErr = errors.New("Invalid acceptance subprocess report")
			} else {
				reported = true
				reportOK = event.Report.OK()
				// The parent builds its report from case events. A failed final
				// report alone must not leave it with only earlier passing cases.
				// Progress updates replace the same case, as they do in the parent.
				if len(results) != len(event.Report.Cases) {
					decodeErr = errors.New("Acceptance subprocess report differs from its case events")
				} else {
					for _, result := range event.Report.Cases {
						key := [2]string{result.Group, result.ID}
						previous, exists := results[key]
						if !exists || !reflect.DeepEqual(previous, result) {
							decodeErr = errors.New("Acceptance subprocess report differs from its case events")
							break
						}
						delete(results, key)
					}
				}
				// A final report ends the suite. Give normal teardown its grace
				// period, then interrupt a child that never exits on its own.
				exitTimer = time.AfterFunc(shutdownDelay, cancel)
				if decodeErr == nil && event.Error != "" {
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
	if decodeErr != nil || !reported {
		cancel()
		// A malformed stream is no longer consumed. Close its reader so exec's
		// copy loop cannot remain blocked while graceful shutdown is awaited.
		_ = output.Close()
	}
	waitErr := <-waited
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
