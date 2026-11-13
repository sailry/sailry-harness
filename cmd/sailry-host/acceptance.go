package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/hostctl"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const acceptanceUsage = `Usage: sailry-host acceptance --data-dir <absolute test profile> [--start] [--suite quick|backend|ui|e2e|full|replay] [--provider <name or id> --model <id> [--effort <level>]] [--case <id> ...] [--timeout <duration>] [--report-dir <directory>] [--desktop <executable>] [--open-report]
       sailry-host acceptance --list [--suite quick|backend]
       sailry-host acceptance-cleanup --data-dir <absolute test profile> [--start] [--report-dir <directory>]
Attaches to the running Host by default. --start exclusively opens the test profile for this run.
Live suites use real models and dedicated test conversations. Configure a separate test profile first.
--provider accepts a unique channel name or provider ID. --effort requires --provider and --model.
UI/E2E/full suites use --desktop for their isolated headless process. Replay opens no Node or model.
Records and reports are private. Cleanup uses recorded ownership only, never session title matching.
Default suite: quick. Default case timeout: 2m. Default report directory: <profile>/acceptance.
Exit codes: 0 passed, 1 failed/blocked/incomplete, 130 canceled. Never runs from CI automatically.`

type acceptanceOptions struct {
	profile, suite, provider, model, reportDirectory, desktop string
	start, list, cleanup, openReport                          bool
	timeout                                                   time.Duration
	cases                                                     []string
	providerID                                                protocol.ProviderID
	effort                                                    *protocol.Effort
}

func parseAcceptance(args []string) (acceptanceOptions, bool, error) {
	options := acceptanceOptions{}
	flags := flag.NewFlagSet("acceptance", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	flags.StringVar(&options.profile, "data-dir", "", "Use this isolated test profile")
	flags.BoolVar(&options.start, "start", false, "Own the profile and stop the Node after testing")
	flags.StringVar(&options.suite, "suite", "quick", "Run quick, backend, ui, e2e, full or replay acceptance")
	flags.StringVar(&options.desktop, "desktop", "", "Use this Desktop executable for headless UI checks")
	flags.BoolVar(&options.cleanup, "cleanup", false, "Clean only recorded acceptance sessions")
	flags.BoolVar(&options.openReport, "open-report", false, "Open the generated HTML report")
	flags.StringVar(&options.provider, "provider", "", "Use this configured channel name or provider ID")
	flags.StringVar(&options.model, "model", "", "Use this configured model ID")
	flags.Func("effort", "Use this protocol reasoning level; requires --provider and --model", func(value string) error {
		effort := protocol.Effort{Kind: value}
		if _, err := effort.MarshalJSON(); err != nil {
			return errors.New("--effort requires a protocol effort level, such as medium")
		}
		options.effort = &effort
		return nil
	})
	flags.StringVar(&options.reportDirectory, "report-dir", "", "Write the report in this private directory")
	flags.DurationVar(&options.timeout, "timeout", 2*time.Minute, "Limit each acceptance case")
	flags.BoolVar(&options.list, "list", false, "List cases without opening a profile or calling a model")
	flags.Func("case", "Run this case ID; repeat to choose more", func(value string) error {
		if strings.TrimSpace(value) == "" {
			return errors.New("--case requires a case ID")
		}
		options.cases = append(options.cases, value)
		return nil
	})
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return options, true, nil
		}
		return options, false, err
	}
	if flags.NArg() != 0 {
		return options, false, errors.New("unexpected acceptance argument; use --help")
	}
	if !slices.Contains([]string{"quick", "backend", "ui", "e2e", "full", "replay"}, options.suite) {
		return options, false, errors.New("--suite must be quick, backend, ui, e2e, full or replay")
	}
	if options.timeout <= 0 {
		return options, false, errors.New("--timeout must be a positive duration, such as 2m")
	}
	if !options.list && !filepath.IsAbs(options.profile) {
		return options, false, errors.New("acceptance requires --data-dir with an absolute test profile")
	}
	if (options.provider == "") != (options.model == "") {
		return options, false, errors.New("choose both --provider and --model, or neither")
	}
	if options.effort != nil && options.provider == "" {
		return options, false, errors.New("--effort requires both --provider and --model")
	}
	if options.provider != "" {
		options.providerID, _ = protocol.ParseProviderID(options.provider)
		if options.provider == "00000000-0000-0000-0000-000000000000" {
			return options, false, errors.New("--provider requires a channel name or nonzero provider ID")
		}
	}
	if options.cleanup && (options.provider != "" || len(options.cases) != 0 || options.list) {
		return options, false, errors.New("cleanup cannot select models or test cases")
	}
	if options.cleanup && (options.suite == "ui" || options.suite == "replay") {
		return options, false, errors.New("cleanup requires the test Node, not an offline suite")
	}
	if options.reportDirectory == "" && !options.list {
		options.reportDirectory = filepath.Join(options.profile, "acceptance")
	}
	if options.reportDirectory != "" {
		absolute, err := filepath.Abs(options.reportDirectory)
		if err != nil {
			return options, false, err
		}
		options.reportDirectory = absolute
	}
	return options, false, nil
}

type acceptanceEvent struct {
	Type   string                  `json:"type"`
	Suite  string                  `json:"suite,omitempty"`
	Case   *acceptance.CaseResult  `json:"case,omitempty"`
	Report *acceptance.Report      `json:"report,omitempty"`
	Files  *acceptance.ReportPaths `json:"files,omitempty"`
	Error  string                  `json:"error,omitempty"`
}

func runAcceptance(ctx context.Context, args []string, output io.Writer) (int, error) {
	options, help, err := parseAcceptance(args)
	if err != nil {
		return 1, err
	}
	if help {
		_, err := fmt.Fprintln(output, acceptanceUsage)
		return 0, err
	}
	definitions, err := acceptance.ListBackendCases("backend")
	if err != nil {
		return 1, err
	}
	for _, id := range options.cases {
		if !slices.ContainsFunc(definitions, func(c acceptance.CaseResult) bool { return c.ID == id }) && !strings.HasPrefix(id, "ui.") && !slices.Contains([]string{"E2E01", "E2E02", "E2E03"}, id) {
			return 1, fmt.Errorf("unknown acceptance case %q", id)
		}
		if !options.list {
			uiCase, liveCase := strings.HasPrefix(id, "ui."), strings.HasPrefix(id, "E2E")
			if options.suite == "backend" && (uiCase || liveCase) || options.suite == "ui" && !uiCase || options.suite == "e2e" && !liveCase || options.suite == "replay" || options.suite == "quick" && options.desktop == "" && (uiCase || liveCase) {
				return 1, fmt.Errorf("case %s is not included in the selected suite", id)
			}
		}
	}
	encoder := json.NewEncoder(output)
	if options.list {
		if len(options.cases) == 0 {
			definitions, err = acceptance.ListBackendCases(options.suite)
			if err != nil {
				return 1, err
			}
		}
		for _, definition := range definitions {
			if len(options.cases) != 0 && !slices.Contains(options.cases, definition.ID) {
				continue
			}
			if err := encoder.Encode(acceptanceEvent{Type: "case", Case: &definition}); err != nil {
				return 1, err
			}
		}
		return 0, nil
	}
	if !options.cleanup && slices.Contains([]string{"ui", "e2e", "full", "replay"}, options.suite) && options.desktop == "" {
		return 1, errors.New("the selected suite requires --desktop with the headless Desktop executable")
	}
	// Reuse the profile's advisory lock in a private runner directory. This
	// directory owns the CLI run and report, and never starts a Node itself.
	lock, err := core.AcquireProfile(filepath.Join(options.reportDirectory, "runner"))
	if err != nil {
		return 1, fmt.Errorf("acceptance report is already in use or unavailable: %w", err)
	}
	defer lock.Close()
	if err := encoder.Encode(acceptanceEvent{Type: "started", Suite: options.suite}); err != nil {
		return 1, err
	}
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	report := acceptance.Report{Version: 1, Suite: options.suite, Started: time.Now().UTC(), Provider: options.provider, Model: options.model, Effort: options.effort, Cases: []acceptance.CaseResult{}}
	var streamErr error
	emit := func(result acceptance.CaseResult) {
		if index := slices.IndexFunc(report.Cases, func(c acceptance.CaseResult) bool { return c.ID == result.ID && c.Group == result.Group }); index >= 0 {
			report.Cases[index] = result
		} else {
			report.Cases = append(report.Cases, result)
		}
		if streamErr == nil {
			streamErr = encoder.Encode(acceptanceEvent{Type: "case", Case: &result})
			if streamErr != nil {
				cancel()
			}
		}
	}
	var controller *client.Client
	var closeBackend func() error
	offline := options.suite == "ui" || options.suite == "replay"
	if !offline {
		controller, closeBackend, err = acceptanceClient(scoped, options)
	}
	defer func() {
		if closeBackend != nil {
			_ = closeBackend()
		}
	}()
	message := ""
	if err != nil {
		message = "Could not connect to the running Host in the selected test profile; start the Host or use --start"
		if options.start {
			message = "Could not start the standalone Node; inspect the test profile and its current owner"
		}
	}
	if err == nil && controller != nil {
		err = acceptance.ImportReportOwnership(options.reportDirectory, controller.Target())
		if err != nil {
			message = "Could not load recorded acceptance ownership: " + err.Error()
		}
	}
	if err == nil && options.cleanup {
		report, err = acceptance.Cleanup(scoped, controller, options.reportDirectory, nil)
		for _, result := range report.Cases {
			if streamErr == nil {
				streamErr = encoder.Encode(acceptanceEvent{Type: "case", Case: &result})
			}
		}
	} else if err == nil {
		var config *protocol.SessionConfig
		if !offline {
			var selected protocol.SessionConfig
			selected, err = acceptanceConfig(controller.Snapshot(), options)
			if err == nil {
				config = &selected
				report.Node = controller.Target()
				report.Provider, report.Model, report.Effort = selected.Provider.String(), selected.Model, &selected.Effort
			}
		}
		runOptions := acceptance.Options{Suite: options.suite, Config: config, Cases: options.cases, CaseTimeout: options.timeout, ReportDirectory: options.reportDirectory}
		if err == nil && !offline && options.suite != "e2e" {
			backendOptions := runOptions
			backendOptions.Cases = nil
			for _, id := range options.cases {
				if !strings.HasPrefix(id, "ui.") && !strings.HasPrefix(id, "E2E") {
					backendOptions.Cases = append(backendOptions.Cases, id)
				}
			}
			if len(options.cases) == 0 || len(backendOptions.Cases) > 0 {
				_, err = acceptance.RunBackend(scoped, controller, backendOptions, emit)
			}
		}
		wantsUI := offline || options.suite == "e2e" || options.suite == "full" || options.suite == "quick" && options.desktop != ""
		if err == nil && wantsUI {
			socket := options.profile
			if offline {
				socket = ""
			}
			err = runAcceptanceChild(scoped, options.desktop, filepath.Join(options.reportDirectory, "controller"), socket, runOptions, emit)
			if err != nil && controller != nil {
				cleanup, stop := context.WithTimeout(context.WithoutCancel(ctx), 30*time.Second)
				var sessions []protocol.SessionID
				for _, result := range report.Cases {
					for _, raw := range append(slices.Clone(result.OwnedSessions), result.Session) {
						if id, e := protocol.ParseSessionID(raw); e == nil && !slices.Contains(sessions, id) {
							sessions = append(sessions, id)
						}
					}
				}
				cleanupErr := cleanupAcceptanceSessions(cleanup, controller, sessions)
				stop()
				err = errors.Join(err, cleanupErr)
			}
		}
	}
	if err == nil {
		for _, id := range options.cases {
			if !slices.ContainsFunc(report.Cases, func(result acceptance.CaseResult) bool { return result.ID == id }) {
				missing := fmt.Errorf("requested case %s did not produce a result", id)
				emit(acceptance.CaseResult{ID: id, Group: "runner", Title: "Run the requested acceptance case", Status: "blocked", Detail: missing.Error()})
				err = errors.Join(err, missing)
			}
		}
	}
	if err != nil && message == "" {
		message = err.Error()
	}
	if err != nil || ctx.Err() != nil {
		status := "blocked"
		if ctx.Err() != nil {
			status, message = "canceled", "Acceptance canceled; inspect the report for cleanup results"
		}
		emit(acceptance.CaseResult{ID: "runner", Group: "runner", Title: "Acceptance run", Status: status, Detail: message})
	}
	report.Finished = time.Now().UTC()
	reportDirectory := options.reportDirectory
	if options.cleanup {
		reportDirectory = filepath.Join(reportDirectory, "cleanup")
	}
	if options.suite == "replay" {
		reportDirectory = filepath.Join(reportDirectory, "replay")
	}
	if options.suite == "ui" {
		reportDirectory = filepath.Join(reportDirectory, "ui")
	}
	paths, reportErr := acceptance.WriteReport(reportDirectory, report)
	if reportErr != nil {
		return 1, reportErr
	}
	// Preserve evidence before removing temporary conversations. A failed
	// publication leaves the recorded sessions available to the cleanup command.
	republish := false
	if controller != nil && !options.cleanup {
		cleanup, stop := context.WithTimeout(context.WithoutCancel(ctx), time.Minute)
		_, cleanupErr := acceptance.Cleanup(cleanup, controller, options.reportDirectory, nil)
		stop()
		if cleanupErr != nil {
			emit(acceptance.CaseResult{ID: "runner.cleanup", Group: "runner", Title: "Acceptance cleanup", Status: "blocked", Detail: cleanupErr.Error(), Uncertain: true})
			err = errors.Join(err, cleanupErr)
			republish = true
		}
	}
	if closeBackend != nil {
		closeErr := closeBackend()
		closeBackend = nil
		if closeErr != nil {
			err = errors.Join(err, closeErr)
			message = "Acceptance Node shutdown failed"
			emit(acceptance.CaseResult{ID: "runner.shutdown", Group: "runner", Title: "Stop the acceptance Node", Status: "blocked", Detail: message, Uncertain: true})
			republish = true
		}
	}
	if republish {
		report.Finished = time.Now().UTC()
		paths, reportErr = acceptance.WriteReport(reportDirectory, report)
		if reportErr != nil {
			return 1, reportErr
		}
	}
	if streamErr != nil {
		return 1, streamErr
	}
	if err := encoder.Encode(acceptanceEvent{Type: "report", Report: &report, Files: &paths, Error: message}); err != nil {
		return 1, err
	}
	if options.openReport {
		if openErr := openAcceptanceReport(paths.HTML); openErr != nil {
			return 1, fmt.Errorf("report saved at %s; could not open it: %w", paths.HTML, openErr)
		}
	}
	if ctx.Err() != nil {
		return 130, nil
	}
	if err != nil || !report.OK() {
		return 1, nil
	}
	return 0, nil
}

func acceptanceConfig(snapshot protocol.Snapshot, options acceptanceOptions) (protocol.SessionConfig, error) {
	var selected *protocol.SessionConfig
	if options.provider != "" {
		matches := []protocol.ConversationProvider{}
		for _, provider := range snapshot.Providers {
			if provider.ID.String() == options.provider || provider.Name == options.provider {
				matches = append(matches, provider)
			}
		}
		if len(matches) != 1 {
			return protocol.SessionConfig{}, errors.New("--provider must identify one configured channel by name or ID")
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = matches[0].ID, options.model
		if options.effort != nil {
			config.Effort = *options.effort
		}
		selected = &config
	}
	return acceptance.SelectConfig(snapshot, selected)
}

func openAcceptanceReport(path string) error {
	var command *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		command = exec.Command("open", path)
	case "windows":
		command = exec.Command("rundll32", "url.dll,FileProtocolHandler", path)
	default:
		command = exec.Command("xdg-open", path)
	}
	return command.Run()
}

func acceptanceClient(ctx context.Context, options acceptanceOptions) (*client.Client, func() error, error) {
	var transport link.Transport
	var execution *node.Node
	var err error
	var server *hostctl.Server
	var stopControl context.CancelFunc
	var controlDone chan error
	if options.start {
		// An explicit standalone run owns only this test profile. It has no
		// Link listener or telemetry consent side effect; model requests still
		// use the Node's normal configured provider and tool implementations.
		execution, err = node.StartWithOptions(ctx, options.profile, node.Options{
			LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities()},
			Telemetry: telemetry.Options{Application: "host", Lookup: func(name string) string {
				if name == "SAILRY_TELEMETRY" {
					return "off"
				}
				return os.Getenv(name)
			}},
		})
		if err != nil {
			return nil, nil, err
		}
		transport = link.NewLocal(execution.ID(), execution.ID(), execution)
		if options.desktop != "" && (options.suite == "full" || options.suite == "quick" || options.suite == "e2e") {
			server, err = hostctl.Listen(options.profile, execution.Link(), transport)
			if err != nil {
				return nil, nil, errors.Join(err, execution.Stop(context.Background()))
			}
			var controlCtx context.Context
			controlCtx, stopControl = context.WithCancel(context.WithoutCancel(ctx))
			controlDone = make(chan error, 1)
			go func() { controlDone <- server.Serve(controlCtx) }()
		}
	} else {
		transport, err = hostctl.Connect(ctx, options.profile)
		if err != nil {
			return nil, nil, err
		}
	}
	controller := client.New(transport)
	closeBackend := func() error {
		if stopControl != nil {
			stopControl()
			_ = server.Close()
			<-controlDone
			stopControl = nil
		}
		controllerErr := controller.Close()
		var nodeErr error
		if execution != nil {
			nodeErr = execution.Stop(context.Background())
		}
		return errors.Join(controllerErr, nodeErr)
	}
	if err := controller.Start(ctx); err != nil {
		return nil, nil, errors.Join(err, closeBackend())
	}
	return controller, closeBackend, nil
}
