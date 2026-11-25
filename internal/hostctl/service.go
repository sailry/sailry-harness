package hostctl

import (
	"context"
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
)

func runHostService(ctx context.Context, args []string, config Options, restoreSignals func()) (int, error) {
	options, help, err := parseService(args, config.Identity.DefaultProfile)
	if err != nil {
		return 1, err
	}
	if help {
		usage := ServiceUsage
		if config.Identity.DefaultProfile != "" {
			usage = strings.Replace(usage, "~/.sailry-go", config.Identity.DefaultProfile, 1)
		}
		_, err := fmt.Fprintln(config.Stdout, usage)
		return 0, err
	}
	if closeLogs, err := logging.Install(options.Profile, "host"); err != nil {
		_, _ = fmt.Fprintln(config.Stderr, config.Identity.HostRelease.Package().Binary()+": continuing without a diagnostic log:", err)
	} else {
		defer closeLogs()
	}
	execution, err := node.StartWithOptions(ctx, options.Profile, node.Options{
		Headless:        true,
		MCP:             &mcp.Options{Warm: true},
		Network:         &options.Network,
		LocalModels:     localmodels.Options{Capabilities: node.LocalModelCapabilities()},
		Telemetry:       telemetry.Options{Product: config.Identity.ID, Version: config.Identity.Version, Application: "host", Lookup: options.Environment},
		HostRelease:     config.Identity.HostRelease,
		ProductID:       config.Identity.ID,
		SoftwareVersion: config.Identity.Version,
		Assembly:        config.Assembly,
	})
	if err != nil {
		return 1, err
	}
	result := serve(ctx, execution, options, config)
	// Restore default signals so another interrupt ends a stuck shutdown.
	restoreSignals()
	shutdown := execution.Stop(context.Background())
	if result != nil {
		if shutdown != nil {
			_, _ = fmt.Fprintln(config.Stderr, "Host shutdown failed: "+shutdown.Error())
		}
		return 1, result
	}
	if shutdown != nil {
		return 1, shutdown
	}
	_, err = fmt.Fprintln(config.Stdout, "Node stopped")
	return 0, err
}

func serve(ctx context.Context, execution *node.Node, options ServiceOptions, config Options) error {
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
		control, err := Listen(options.Profile, endpoint, transport)
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
	if _, err := fmt.Fprintf(config.Stdout, "Node ready: %s (only paired peers are authorized)\nLink address: %s\n", options.Profile, encoded); err != nil {
		return err
	}
	if options.Bootstrap {
		start(func() error { return Bootstrap(scoped, options.Profile, endpoint) })
	}
	if options.PairingService != "" {
		start(func() error {
			return ReportSharing(scoped, endpoint, options.PairingService, config.Stdout, config.Stderr)
		})
	}
	select {
	case <-ctx.Done():
		return nil
	case err := <-failures:
		return err
	}
}
