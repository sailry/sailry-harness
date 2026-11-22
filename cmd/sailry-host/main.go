package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"runtime"
	"sync"
	"syscall"

	"github.com/sailry/sailry-harness/internal/hostctl"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/modules/code-index/node"
)

func main() {
	computer.Trampoline()
	code, err := run(os.Args[1:])
	if err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "sailry-host: "+err.Error())
		code = 1
	}
	os.Exit(code)
}

func run(args []string) (int, error) {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	return hostctl.RunCLI(ctx, args, os.Stdin, os.Stdout, os.Stderr, hostctl.CLIHandlers{
		Service: func(ctx context.Context, args []string) (int, error) {
			return runService(ctx, args, cancel)
		},
		Acceptance: func(ctx context.Context, args []string) (int, error) {
			return runAcceptance(ctx, args, os.Stdout)
		},
	})
}

func runService(ctx context.Context, args []string, cancel context.CancelFunc) (int, error) {
	options, help, err := hostctl.ParseService(args)
	if err != nil {
		return 1, err
	}
	if help {
		_, err := fmt.Fprintln(os.Stdout, hostctl.ServiceUsage)
		return 0, err
	}
	if closeLogs, err := logging.Install(options.Profile, "host"); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "sailry-host: continuing without a diagnostic log:", err)
	} else {
		defer closeLogs()
	}
	execution, err := node.StartWithOptions(ctx, options.Profile, node.Options{
		Headless:    true,
		MCP:         &mcp.Options{Warm: true},
		Network:     &options.Network,
		LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities()},
		Telemetry:   telemetry.Options{Application: "host", Lookup: options.Environment},
		CodeIndex:   codeindex.NewService,
	})
	if err != nil {
		return 1, err
	}
	result := serve(ctx, execution, options)
	// Restore default signals so another interrupt ends a stuck shutdown.
	cancel()
	shutdown := execution.Stop(context.Background())
	if result != nil {
		if shutdown != nil {
			_, _ = fmt.Fprintln(os.Stderr, "Host shutdown failed: "+shutdown.Error())
		}
		return 1, result
	}
	if shutdown != nil {
		return 1, shutdown
	}
	_, err = fmt.Fprintln(os.Stdout, "Node stopped")
	return 0, err
}

func serve(ctx context.Context, execution *node.Node, options hostctl.ServiceOptions) error {
	endpoint := execution.Link()
	if err := endpoint.Online(ctx); err != nil {
		return err
	}
	if _, err := execution.Snapshot(ctx); err != nil {
		return err
	}
	scoped, cancel := context.WithCancel(ctx)
	var owners sync.WaitGroup
	defer func() { cancel(); owners.Wait() }()
	failures := make(chan error, 1)
	start := func(action func() error) {
		owners.Add(1)
		go func() {
			defer owners.Done()
			if err := action(); err != nil && scoped.Err() == nil {
				select {
				case failures <- err:
				default:
				}
			}
		}()
	}
	if runtime.GOOS != "windows" {
		transport := link.NewLocal(execution.ID(), execution.ID(), execution)
		control, err := hostctl.Listen(options.Profile, endpoint, transport)
		if err != nil {
			return err
		}
		defer control.Close()
		start(func() error { return control.Serve(scoped) })
	}
	address, err := endpoint.Address(ctx)
	if err != nil {
		return err
	}
	encoded, err := json.Marshal(address)
	if err != nil {
		return err
	}
	if _, err := fmt.Fprintf(os.Stdout, "Node ready: %s (only paired peers are authorized)\nLink address: %s\n", options.Profile, encoded); err != nil {
		return err
	}
	if options.Bootstrap {
		start(func() error { return hostctl.Bootstrap(scoped, options.Profile, endpoint) })
	}
	if options.PairingService != "" {
		start(func() error {
			return hostctl.ReportSharing(scoped, endpoint, options.PairingService, os.Stdout, os.Stderr)
		})
	}
	select {
	case <-ctx.Done():
		return nil
	case err := <-failures:
		return err
	}
}
