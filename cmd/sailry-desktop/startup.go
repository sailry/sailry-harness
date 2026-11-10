package main

import (
	"context"
	"errors"
	"flag"
	"io"
	"path/filepath"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
)

type startupOptions struct {
	Preview bool
	DataDir string
}

func parseOptions(args []string) (startupOptions, error) {
	var options startupOptions
	flags := flag.NewFlagSet("sailry-desktop", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	flags.BoolVar(&options.Preview, "preview", false, "Show the isolated preview without starting a Node")
	flags.StringVar(&options.DataDir, "data-dir", "", "Use this Node profile directory")
	if err := flags.Parse(args); err != nil {
		return options, err
	}
	if flags.NArg() != 0 {
		return options, errors.New("unexpected desktop argument")
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
	err         error
}

func openBackend(ctx context.Context, options startupOptions) (*desktopBackend, error) {
	backend := &desktopBackend{}
	if options.Preview {
		return backend, nil
	}
	path := options.DataDir
	var err error
	if path == "" {
		path, err = core.DefaultDataDir()
	} else {
		path, err = filepath.Abs(path)
	}
	if err != nil {
		return nil, err
	}
	// First-launch detection must precede creation of the Node database.
	backend.Path, backend.Preferences = path, preferences.ForStartup(path)
	backend.Node, err = node.StartWithOptions(ctx, path, node.Options{
		Network:     &link.NetworkScope{Internet: true},
		LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities()},
	})
	if err != nil {
		return nil, err
	}
	backend.Client = client.New(link.NewLocal(backend.Node.ID(), backend.Node.ID(), backend.Node))
	if err = backend.Client.Start(ctx); err != nil {
		return nil, errors.Join(err, backend.Close())
	}
	backend.Network = live.NewNetwork(ctx, backend.Client, backend.Node.Link())
	return backend, nil
}

func (backend *desktopBackend) Close() error {
	backend.close.Do(func() {
		var network, controller, nodeErr error
		if backend.Network != nil {
			network = backend.Network.Close()
		}
		if backend.Client != nil {
			controller = backend.Client.Close()
		}
		if backend.Node != nil {
			nodeErr = backend.Node.Stop(context.Background())
		}
		backend.err = errors.Join(network, controller, nodeErr)
	})
	return backend.err
}
