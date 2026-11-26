package application

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type startupOptions struct {
	Preview             bool
	DataDir             string
	AcceptanceUI        bool
	AcceptanceDirectory string
	AcceptanceSocket    string
	AcceptanceMode      string
	AcceptanceReplay    string
	Relays              []string
}

func parseOptions(args []string) (startupOptions, error) {
	var options startupOptions
	flags := flag.NewFlagSet("sailry-desktop", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	flags.BoolVar(&options.Preview, "preview", false, "Show the isolated preview without starting a Node")
	flags.Func("data-dir", "Use this Node profile directory", func(value string) error {
		if value == "" {
			return errors.New("--data-dir requires a directory")
		}
		options.DataDir = value
		return nil
	})
	flags.BoolVar(&options.AcceptanceUI, "acceptance-ui", false, "Run explicit headless conversation acceptance without opening a native window")
	flags.StringVar(&options.AcceptanceDirectory, "acceptance-dir", "", "Use this isolated acceptance controller directory")
	flags.StringVar(&options.AcceptanceSocket, "acceptance-socket", "", "Attach live acceptance to this private Node control directory")
	flags.StringVar(&options.AcceptanceMode, "acceptance-mode", "", "Choose the headless acceptance scope")
	flags.StringVar(&options.AcceptanceReplay, "acceptance-replay", "", "Replay recorded acceptance output without a Node")
	flags.Func("relay", "Start with this libp2p relay multiaddr; repeat for a second relay", func(value string) error {
		options.Relays = append(options.Relays, value)
		return nil
	})
	if err := flags.Parse(args); err != nil {
		return options, err
	}
	if flags.NArg() != 0 {
		return options, errors.New("unexpected desktop argument")
	}
	if options.AcceptanceUI {
		if options.AcceptanceDirectory == "" || options.Preview || options.DataDir != "" || len(options.Relays) != 0 {
			return options, errors.New("--acceptance-ui requires --acceptance-dir and cannot open a Node profile or preview")
		}
		if options.AcceptanceMode != "" && !slices.Contains([]string{"quick", "ui", "e2e", "full", "replay"}, options.AcceptanceMode) {
			return options, errors.New("invalid headless acceptance scope")
		}
		if options.AcceptanceReplay != "" && (options.AcceptanceSocket != "" || options.AcceptanceMode != "replay") {
			return options, errors.New("acceptance replay cannot connect to a Node")
		}
		switch options.AcceptanceMode {
		case "", "ui":
			if options.AcceptanceSocket != "" {
				return options, errors.New("fixture acceptance cannot connect to a Node")
			}
		case "replay":
			if options.AcceptanceReplay == "" {
				return options, errors.New("acceptance replay requires a recordings directory")
			}
		case "quick", "full", "e2e":
			if options.AcceptanceSocket == "" {
				return options, errors.New("live acceptance requires a Node control directory")
			}
		}
	} else if options.AcceptanceDirectory != "" || options.AcceptanceSocket != "" || options.AcceptanceMode != "" || options.AcceptanceReplay != "" {
		return options, errors.New("acceptance options require --acceptance-ui")
	}
	if len(options.Relays) > 0 {
		relays, err := link.NetworkRelayURLs(context.Background(), protocol.NetworkSelection{
			Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: options.Relays},
		})
		if err != nil {
			return options, fmt.Errorf("--relay: %w", err)
		}
		options.Relays = relays
	}
	return options, nil
}

type desktopBackend struct {
	Path        string
	Node        *node.Node
	Client      *client.Client
	Network     *live.Network
	Preferences *preferences.Store
	close       sync.Once
	// stopped is set once the backend was closed for a successor.
	stopped atomic.Bool
	logs    func()
	reports *interfaceReports
	err     error
}

func openBackend(ctx context.Context, options startupOptions, product identity.Desktop, hostAssembly moduleapi.Assembly) (*desktopBackend, error) {
	backend := &desktopBackend{}
	if options.Preview {
		return backend, nil
	}
	path, err := product.DataDir(options.DataDir)
	if err != nil {
		return nil, err
	}
	if options.DataDir == "" {
		if err := node.MigrateLegacyProfile(ctx, product.LegacyProfile, path); err != nil {
			return nil, err
		}
	}
	// First-launch detection must precede creation of the Node database.
	backend.Path, backend.Preferences = path, preferences.ForStartup(path)
	if backend.logs, err = logging.Install(path, "desktop"); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "sailry-desktop: continuing without a diagnostic log:", err)
	}
	backend.Node, err = node.StartWithOptions(ctx, path, node.Options{
		MCP:             &mcp.Options{Warm: true},
		Network:         startupScope(options.Relays),
		LocalModels:     localmodels.Options{Capabilities: node.LocalModelCapabilities()},
		HostRelease:     product.HostRelease,
		ProductID:       product.ID,
		SoftwareVersion: product.Version,
		Telemetry:       telemetry.Options{Product: product.ID, Version: product.Version},
		Assembly:        hostAssembly,
	})
	if err != nil {
		backend.closeLogs()
		return nil, err
	}
	// Only the profile owner may consume or truncate its previous crash trace.
	if backend.logs != nil {
		backend.reports = catchCrashes()
		backend.reports.start(backend.Node.Telemetry())
	}
	backend.Client = client.New(link.NewLocal(backend.Node.ID(), backend.Node.ID(), backend.Node))
	if err = backend.Client.Start(ctx); err != nil {
		return nil, errors.Join(err, backend.Close())
	}
	backend.Network = live.NewNetwork(ctx, backend.Client, backend.Node.Link())
	return backend, nil
}

// startupScope listens directly unless --relay was given. A supported
// selection saved on the Node takes precedence.
func startupScope(relays []string) *link.NetworkScope {
	if len(relays) == 0 {
		return &link.NetworkScope{Internet: true}
	}
	return &link.NetworkScope{Relays: slices.Clone(relays)}
}

func (backend *desktopBackend) updates(product identity.Desktop, quit func()) *live.Updates {
	if backend.Client == nil || backend.Preferences == nil {
		return nil
	}
	return live.NewUpdates(product, backend.Preferences, backend.Path, backend.Stop, quit)
}

func (backend *desktopBackend) closeLogs() {
	if backend.logs != nil {
		backend.logs()
	}
}

func (backend *desktopBackend) Close() error {
	backend.close.Do(func() {
		var network, controller, nodeErr error
		backend.reports.close()
		if backend.Network != nil {
			network = backend.Network.Close()
		}
		if backend.Client != nil {
			controller = backend.Client.Close()
		}
		if backend.Node != nil {
			nodeErr = backend.Node.Stop(context.Background())
		}
		backend.closeLogs()
		backend.err = errors.Join(network, controller, nodeErr)
	})
	return backend.err
}

// Stop closes the backend for the updater, which replaces or restarts the
// application next. A shutdown that passed its deadline has released the
// profile, and the exit that follows ends the work that did not stop.
func (backend *desktopBackend) Stop() error {
	// The application started next inherits the mark and waits for this one
	// to quit instead of handing its launch over to it.
	backend.stopped.Store(true)
	_ = os.Setenv(successorVariable, "1")
	err := backend.Close()
	if errors.Is(err, core.ErrShutdownTimeout) {
		slog.Error("Node shutdown", "error", err.Error())
		return nil
	}
	return err
}
