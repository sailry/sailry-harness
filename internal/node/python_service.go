package node

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) initPython(ctx context.Context, configured *python.Options) error {
	options := python.Options{}
	if configured != nil {
		options = *configured
	}
	if options.Environment == nil {
		options.Environment = slices.Clone(node.environment.TerminalValues())
		slices.Sort(options.Environment)
	}
	changed := options.Changed
	options.Changed = func(values []protocol.PythonPreparation) {
		node.plugins.mu.Lock()
		node.plugins.preparations = slices.Clone(values)
		close(node.plugins.pythonWake)
		node.plugins.pythonWake = make(chan struct{})
		node.plugins.mu.Unlock()
		if changed != nil {
			changed(values)
		}
	}
	node.plugins.pythonWake = make(chan struct{})
	manager, err := python.NewManager(node.profile.Ownership.Path, options)
	if err != nil {
		return err
	}
	node.plugins.python = manager
	var settings protocol.PythonSettings
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		settings, err = store.ReadPythonSettings(ctx, db)
		return err
	})
	if err != nil {
		return err
	}
	if err := manager.Configure(settings.Index); err != nil {
		return err
	}
	return node.retainPackagePython(ctx)
}

func (node *Node) packageRequirements(ctx context.Context, info protocol.PluginInfo) (*python.Requirements, error) {
	if info.Skill != nil {
		return nil, nil
	}
	reference := plugins.PackageReference(info)
	content, err := node.plugins.manager.ReadResource(ctx, reference, "requirements.txt", 512*1024)
	var fault *protocol.Fault
	if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return python.NewRequirements(reference, string(content))
}

func (node *Node) preparePackagePython(ctx context.Context, info protocol.PluginInfo) (func() error, error) {
	requirements, err := node.packageRequirements(ctx, info)
	if err != nil || requirements == nil {
		return nil, err
	}
	ready, err := node.plugins.python.Ensure(ctx, requirements)
	if err != nil {
		return nil, err
	}
	return ready.Close, nil
}

func (node *Node) retainPackagePython(ctx context.Context) error {
	var infos []protocol.PluginInfo
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		infos, err = store.ReadPluginInfos(ctx, db)
		return err
	}); err != nil {
		return err
	}
	var retained []python.Requirements
	for _, info := range infos {
		requirements, err := node.packageRequirements(ctx, info)
		if err != nil {
			return err
		}
		if requirements != nil {
			retained = append(retained, *requirements)
		}
	}
	node.plugins.python.Retain(retained)
	return nil
}

type pythonEvents struct {
	node    *Node
	initial bool
	wake    <-chan struct{}
	closed  chan struct{}
	once    sync.Once
	next    sync.Mutex
}

func (stream *pythonEvents) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

func (stream *pythonEvents) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	if !stream.initial {
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-stream.node.closed:
			return protocol.Update{}, unavailable()
		case <-stream.closed:
			return protocol.Update{}, unavailable()
		case <-stream.wake:
		}
	}
	stream.node.plugins.mu.Lock()
	defer stream.node.plugins.mu.Unlock()
	stream.initial = false
	stream.wake = stream.node.plugins.pythonWake
	values := slices.Clone(stream.node.plugins.preparations)
	if values == nil {
		values = []protocol.PythonPreparation{}
	}
	return protocol.Update{Kind: "python", Data: protocol.UpdatePythonPayload{Node: stream.node.ID(), Preparations: values}}, nil
}
