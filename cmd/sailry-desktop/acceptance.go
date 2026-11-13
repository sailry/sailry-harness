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
	"os/signal"
	"path/filepath"
	"runtime"
	"slices"
	"sync"
	"syscall"
	"time"

	"github.com/egoist/mygo"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	profile "github.com/sailry/sailry-harness/internal/core"
	desktop "github.com/sailry/sailry-harness/internal/desktop/acceptance"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
	"github.com/sailry/sailry-harness/internal/hostctl"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type acceptanceEvent struct {
	Kind   string           `json:"kind"`
	Case   *core.CaseResult `json:"case,omitempty"`
	Report *core.Report     `json:"report,omitempty"`
	Error  string           `json:"error,omitempty"`
}

// desktopAcceptance is shared by every window. A profile has one acceptance
// owner, keeping its private socket and latest report from overlapping runs.
type desktopAcceptance struct {
	backend   *desktopBackend
	mu        sync.Mutex
	uncertain bool
}

func (owner *desktopAcceptance) run(ctx context.Context, options core.Options, emit func(core.CaseResult)) (report core.Report, paths core.ReportPaths, resultErr error) {
	report = core.Report{Version: 1, Suite: options.Suite, Started: time.Now().UTC(), Cases: []core.CaseResult{}}
	add := func(result core.CaseResult) {
		if index := slices.IndexFunc(report.Cases, func(old core.CaseResult) bool { return old.ID == result.ID && old.Group == result.Group }); index >= 0 {
			report.Cases[index] = result
		} else {
			report.Cases = append(report.Cases, result)
		}
		if emit != nil {
			emit(result)
		}
	}
	issue := func(id, group string, err error) {
		status := "blocked"
		if ctx.Err() != nil {
			status = "canceled"
		}
		add(core.CaseResult{ID: id, Group: group, Title: "Start acceptance", Status: status, Detail: err.Error()})
	}
	if !owner.mu.TryLock() {
		err := errors.New("Acceptance is already running in another window")
		issue("runner.busy", "ui", err)
		report.Finished = time.Now().UTC()
		return report, paths, err
	}
	defer owner.mu.Unlock()
	publish := false
	defer func() {
		report.Finished = time.Now().UTC()
		for _, result := range report.Cases {
			owner.uncertain = owner.uncertain || result.Uncertain
		}
		if publish {
			var err error
			paths, err = core.WriteReport(filepath.Join(owner.backend.Path, "desktop", "acceptance"), report)
			resultErr = errors.Join(resultErr, err)
		}
	}()
	if owner.backend == nil || owner.backend.Node == nil || owner.backend.Client == nil {
		err := errors.New("Acceptance requires the local execution Node")
		issue("runner.node", "backend", err)
		return report, paths, err
	}
	if owner.uncertain {
		err := errors.New("Inspect the previous unconfirmed acceptance report before restarting Desktop and running acceptance again")
		add(core.CaseResult{ID: "runner.uncertain", Group: "backend", Title: "Start acceptance", Status: "blocked", Detail: err.Error(), Uncertain: true})
		return report, paths, err
	}
	// The same owner holds execution and publication. A rejected second
	// window cannot replace the latest files of an actual acceptance run.
	publish = true
	if options.Config != nil {
		report.Provider, report.Model = options.Config.Provider.String(), options.Config.Model
		report.Effort = &options.Config.Effort
	}
	if options.Suite != "ui" {
		backend, err := core.RunBackend(ctx, owner.backend.Client, options, add)
		report.Provider, report.Model = backend.Provider, backend.Model
		report.Effort = backend.Effort
		if err != nil {
			issue("runner.backend", "backend", err)
			return report, paths, err
		}
	}
	if options.Suite == "backend" {
		return report, paths, nil
	}
	if err := ctx.Err(); err != nil {
		issue("runner.ui", "ui", err)
		return report, paths, err
	}
	directory := filepath.Join(owner.backend.Path, "desktop", "acceptance", "controller")
	socket := ""
	if options.Suite != "ui" {
		// This is another controller of the existing Node, never a second Node
		// or a copy of its credentials. Keep the socket alive through child
		// cancellation so its owned-session cleanup can still reach the Node.
		socket = filepath.Join(owner.backend.Path, "tmp", "acceptance-control")
		if err := profile.PrivateDirectory(socket); err != nil {
			issue("runner.control", "e2e", err)
			return report, paths, err
		}
		transport := link.NewLocal(owner.backend.Node.ID(), owner.backend.Node.ID(), owner.backend.Node)
		server, err := hostctl.Listen(socket, owner.backend.Node.Link(), transport)
		if err != nil {
			issue("runner.control", "e2e", err)
			return report, paths, err
		}
		controlCtx, stop := context.WithCancel(context.WithoutCancel(ctx))
		done := make(chan error, 1)
		go func() { done <- server.Serve(controlCtx) }()
		defer func() {
			stop()
			_ = server.Close()
			if err := <-done; err != nil {
				resultErr = errors.Join(resultErr, err)
			}
		}()
	}
	var sessions []protocol.SessionID
	childResult := func(result core.CaseResult) {
		if result.Group == "e2e" && result.Session != "" {
			if id, err := protocol.ParseSessionID(result.Session); err == nil && !slices.Contains(sessions, id) {
				sessions = append(sessions, id)
			}
		}
		add(result)
	}
	err := runAcceptanceChild(ctx, directory, socket, options.Config, childResult)
	if err != nil {
		issue("runner.ui", "ui", err)
		// A controller disconnect does not stop admitted work. The child
		// reports ownership before sending, so the parent can finish cleanup
		// after a crash, malformed output or forced process termination.
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 15*time.Second)
		cleanupErr := cleanupAcceptanceSessions(cleanup, owner.backend.Client, sessions)
		cancel()
		if cleanupErr != nil {
			add(core.CaseResult{ID: "runner.cleanup", Group: "e2e", Title: "Acceptance cleanup", Status: "blocked", Detail: cleanupErr.Error(), Uncertain: true})
			err = errors.Join(err, cleanupErr)
		}
	}
	return report, paths, err
}

func runAcceptanceChild(ctx context.Context, directory, socket string, config *protocol.SessionConfig, emit func(core.CaseResult)) error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	args := []string{"--acceptance-ui", "--acceptance-dir", directory}
	if socket != "" {
		args = append(args, "--acceptance-socket", socket)
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
	input, err := json.Marshal(config)
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
		var event acceptanceEvent
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

// runAcceptanceUI branches before instance locks, profiles and native windows.
// Fixture tests and live tests run here because rendering updates process-wide
// theme state, which must not race with the user's real Desktop window.
func runAcceptanceUI(options startupOptions) error {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	directory, err := filepath.Abs(options.AcceptanceDirectory)
	if err != nil {
		return err
	}
	if err := profile.PrivateDirectory(directory); err != nil {
		return err
	}
	mygo.App.SetPath(mygo.PathUserData, directory)
	mygo.App.SetLanguages(i18n.Languages...)
	crychic.Setup()
	encoder := json.NewEncoder(os.Stdout)
	var outputErr error
	emit := func(result core.CaseResult) {
		if outputErr == nil {
			outputErr = encoder.Encode(acceptanceEvent{Kind: "case", Case: &result})
		}
	}
	report := desktop.Run(ctx, emit)
	var runErr error
	if options.AcceptanceSocket != "" && ctx.Err() == nil {
		var config *protocol.SessionConfig
		if err := json.NewDecoder(io.LimitReader(os.Stdin, protocol.MAX_FRAME_BYTES)).Decode(&config); err != nil || config == nil {
			runErr = errors.New("Live acceptance requires a model configuration on stdin")
		} else {
			var transport link.Transport
			transport, runErr = hostctl.Connect(ctx, options.AcceptanceSocket)
			if runErr == nil {
				controller := client.New(transport)
				defer controller.Close()
				runErr = controller.Start(ctx)
				if runErr == nil {
					live := desktop.RunLive(ctx, controller, config, emit, directory)
					report.Suite, report.Provider, report.Model = "ui+e2e", live.Provider, live.Model
					report.Effort = live.Effort
					report.Cases = append(report.Cases, live.Cases...)
					report.Finished = live.Finished
				}
			}
		}
	}
	if runErr != nil {
		result := core.CaseResult{ID: "e2e.setup", Group: "e2e", Title: "Connect the UI controller", Status: "blocked", Detail: runErr.Error()}
		report.Cases = append(report.Cases, result)
		emit(result)
	}
	if options.AcceptanceSocket == "" {
		_, runErr = core.WriteReport(filepath.Join(directory, "report"), report)
	}
	message := ""
	if runErr != nil {
		message = runErr.Error()
	}
	if err := encoder.Encode(acceptanceEvent{Kind: "report", Report: &report, Error: message}); err != nil {
		return err
	}
	if err := errors.Join(outputErr, runErr, ctx.Err()); err != nil {
		return err
	}
	if !report.OK() {
		return errors.New("Conversation acceptance found failed, blocked or incomplete cases")
	}
	return nil
}
