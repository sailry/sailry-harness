package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"slices"
	"syscall"

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
			if outputErr != nil {
				cancel()
			}
		}
	}
	runOptions := core.Options{Suite: "ui", ReportDirectory: filepath.Join(directory, "report")}
	var runErr error
	if options.AcceptanceMode != "" {
		runErr = json.NewDecoder(io.LimitReader(os.Stdin, protocol.MAX_FRAME_BYTES)).Decode(&runOptions)
		if runErr != nil {
			runErr = errors.New("Headless acceptance requires options on stdin")
		} else if runOptions.Suite != options.AcceptanceMode {
			runErr = errors.New("Headless acceptance scope differs from its input options")
		}
	}
	report := core.Report{Version: 1, Suite: runOptions.Suite}
	if runErr == nil && options.AcceptanceReplay != "" {
		report = desktop.RunReplay(ctx, options.AcceptanceReplay, emit)
	} else if runErr == nil {
		if options.AcceptanceMode != "e2e" {
			report = desktop.RunCases(ctx, runOptions.Cases, emit)
		}
		if slices.Contains([]string{"quick", "full", "e2e"}, options.AcceptanceMode) && options.AcceptanceSocket != "" && ctx.Err() == nil {
			if runOptions.Config == nil {
				runErr = errors.New("Live acceptance requires a model configuration")
			} else {
				var transport link.Transport
				transport, runErr = hostctl.Connect(ctx, options.AcceptanceSocket)
				if runErr == nil {
					controller := client.New(transport)
					defer controller.Close()
					runErr = controller.Start(ctx)
					if runErr == nil {
						liveReport := desktop.RunLiveWithOptions(ctx, controller, runOptions, emit, directory)
						if report.Started.IsZero() {
							report.Started = liveReport.Started
						}
						report.Suite, report.Node, report.Provider, report.Model = "ui+e2e", liveReport.Node, liveReport.Provider, liveReport.Model
						report.Effort = liveReport.Effort
						report.Cases = append(report.Cases, liveReport.Cases...)
						report.Finished = liveReport.Finished
					}
				}
			}
		}
	}
	if runErr != nil {
		result := core.CaseResult{ID: "e2e.setup", Group: "e2e", Title: "Connect the UI controller", Status: "blocked", Detail: runErr.Error()}
		report.Cases = append(report.Cases, result)
		emit(result)
	}
	if options.AcceptanceMode == "" {
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
