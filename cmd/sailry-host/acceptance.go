package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
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

const acceptanceUsage = `Usage: sailry-host acceptance --data-dir <absolute test profile> [--start] [--suite quick|backend] [--provider <id> --model <id> [--effort <level>]] [--case <id> ...] [--timeout <duration>] [--report-dir <directory>]
       sailry-host acceptance --list [--suite quick|backend]
Attaches to the running Host by default. --start exclusively opens the test profile for this run.
Uses real models and dedicated test conversations. Configure a separate test profile before running.
--effort chooses an explicit protocol reasoning level, such as medium; it requires --provider and --model.
Default suite: quick. Default timeout per case: 2m. Default report directory: <profile>/acceptance.
Writes JSON lines for case results and the final report; saves private latest.json and latest.html.
Exit codes: 0 passed, 1 failed/blocked/incomplete, 130 canceled. Never runs from CI automatically.`

type acceptanceOptions struct {
	profile, suite, provider, model, reportDirectory string
	start, list                                      bool
	timeout                                          time.Duration
	cases                                            []string
	providerID                                       protocol.ProviderID
	effort                                           *protocol.Effort
}

func parseAcceptance(args []string) (acceptanceOptions, bool, error) {
	options := acceptanceOptions{}
	flags := flag.NewFlagSet("acceptance", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	flags.StringVar(&options.profile, "data-dir", "", "Use this isolated test profile")
	flags.BoolVar(&options.start, "start", false, "Own the profile and stop the Node after testing")
	flags.StringVar(&options.suite, "suite", "quick", "Run quick or backend acceptance")
	flags.StringVar(&options.provider, "provider", "", "Use this configured provider ID")
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
	if options.suite != "quick" && options.suite != "backend" {
		return options, false, errors.New("--suite must be quick or backend")
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
		provider, err := protocol.ParseProviderID(options.provider)
		if err != nil {
			return options, false, errors.New("--provider requires a valid provider ID")
		}
		options.providerID = provider
	}
	if options.reportDirectory == "" && !options.list {
		options.reportDirectory = filepath.Join(options.profile, "acceptance")
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
		if !slices.ContainsFunc(definitions, func(value acceptance.CaseResult) bool { return value.ID == id }) {
			return 1, fmt.Errorf("unknown backend acceptance case %q", id)
		}
	}
	encoder := json.NewEncoder(output)
	if options.list {
		cases := definitions
		if len(options.cases) == 0 {
			cases, err = acceptance.ListBackendCases(options.suite)
			if err != nil {
				return 1, err
			}
		}
		for _, definition := range cases {
			if len(options.cases) != 0 && !slices.Contains(options.cases, definition.ID) {
				continue
			}
			if err := encoder.Encode(acceptanceEvent{Type: "case", Case: &definition}); err != nil {
				return 1, err
			}
		}
		return 0, nil
	}
	if err := encoder.Encode(acceptanceEvent{Type: "started", Suite: options.suite}); err != nil {
		return 1, err
	}
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	report := acceptance.Report{Version: 1, Suite: options.suite, Started: time.Now().UTC(), Provider: options.provider, Model: options.model, Effort: options.effort, Cases: []acceptance.CaseResult{}}
	controller, closeBackend, err := acceptanceClient(scoped, options)
	var streamErr error
	emit := func(result acceptance.CaseResult) {
		if streamErr == nil {
			streamErr = encoder.Encode(acceptanceEvent{Type: "case", Case: &result})
			if streamErr != nil {
				cancel()
			}
		}
	}
	message := ""
	if err != nil {
		message = "Could not connect to the running Host in the selected test profile; start the Host or use --start"
		if options.start {
			message = "Could not start the standalone Node; inspect the test profile and its current owner"
			var inUse *core.ProfileInUseError
			if errors.As(err, &inUse) {
				message = "The test profile already has a Node owner; omit --start to attach to its running Host"
			}
		}
	}
	if err == nil {
		var config *protocol.SessionConfig
		if options.provider != "" {
			selected := protocol.DefaultSessionConfig()
			selected.Provider = options.providerID
			selected.Model = options.model
			if options.effort != nil {
				selected.Effort = *options.effort
			}
			config = &selected
		}
		report, err = acceptance.RunBackend(scoped, controller, acceptance.Options{
			Suite: options.suite, Config: config, Cases: options.cases, CaseTimeout: options.timeout,
		}, emit)
		if err != nil {
			// RunBackend returns test-owned validation errors and context
			// errors; provider response bodies stay in its case sanitizer.
			message = err.Error()
		}
	}
	if closeBackend != nil {
		if closeErr := closeBackend(); closeErr != nil {
			err = errors.Join(err, closeErr)
			message = "Acceptance cleanup is unconfirmed; inspect the test profile before another run"
			result := acceptance.CaseResult{ID: "cleanup", Group: "backend", Title: "Acceptance cleanup", Status: "blocked", Detail: message}
			report.Cases = append(report.Cases, result)
			emit(result)
		}
	}
	report.Finished = time.Now().UTC()
	if ctx.Err() != nil {
		message = "Acceptance canceled; inspect the report for cleanup results"
	}
	if err != nil || ctx.Err() != nil {
		if len(report.Cases) == 0 {
			status := "blocked"
			if ctx.Err() != nil {
				status = "canceled"
			}
			result := acceptance.CaseResult{ID: "setup", Group: "backend", Title: "Acceptance setup", Status: status, Detail: message}
			report.Cases = append(report.Cases, result)
			emit(result)
		}
	}
	if ctx.Err() != nil && report.OK() {
		result := acceptance.CaseResult{ID: "canceled", Group: "backend", Title: "Acceptance canceled", Status: "canceled", Detail: "The run was canceled before its lifecycle completed"}
		report.Cases = append(report.Cases, result)
		emit(result)
	}
	paths, reportErr := acceptance.WriteReport(options.reportDirectory, report)
	if reportErr != nil {
		return 1, reportErr
	}
	if streamErr != nil {
		return 1, streamErr
	}
	if err := encoder.Encode(acceptanceEvent{Type: "report", Report: &report, Files: &paths, Error: message}); err != nil {
		return 1, err
	}
	if ctx.Err() != nil {
		return 130, nil
	}
	if err != nil || !report.OK() {
		return 1, nil
	}
	return 0, nil
}

func acceptanceClient(ctx context.Context, options acceptanceOptions) (*client.Client, func() error, error) {
	var transport link.Transport
	var execution *node.Node
	var err error
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
	} else {
		transport, err = hostctl.Connect(ctx, options.profile)
		if err != nil {
			return nil, nil, err
		}
	}
	controller := client.New(transport)
	closeBackend := func() error {
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
