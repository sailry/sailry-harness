package node

import (
	"context"
	"database/sql"
	"errors"
	"io/fs"
	"log/slog"
	"maps"
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
	return node.retainPackagePython(ctx, true)
}

func (node *Node) packageRequirements(ctx context.Context, info protocol.PluginInfo) (*python.Requirements, error) {
	if info.Skill != nil {
		return nil, nil
	}
	reference := plugins.PackageReference(info)
	content, err := node.plugins.manager.ReadResource(ctx, reference, "requirements.txt", 512*1024)
	if errors.Is(err, plugins.ErrResourceMissing) {
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

func (node *Node) retainPackagePython(ctx context.Context, startup bool) error {
	node.plugins.inventoryMu.Lock()
	defer node.plugins.inventoryMu.Unlock()
	var infos []protocol.PluginInfo
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		users, err := store.ListUsers(ctx, db)
		if err != nil {
			return err
		}
		for _, user := range users {
			if user.State != "active" {
				continue
			}
			owned, err := store.ReadPluginInfos(store.WithCapturedUser(store.WithUser(ctx, user), user.ID), db)
			if err != nil {
				return err
			}
			infos = append(infos, owned...)
		}
		return nil
	}); err != nil {
		return err
	}
	var retained []python.Requirements
	node.plugins.mu.Lock()
	unavailable := maps.Clone(node.plugins.unavailable)
	node.plugins.mu.Unlock()
	if unavailable == nil {
		unavailable = make(map[string]struct{})
	}
	installed := make(map[string]protocol.PluginInfo, len(infos))
	for _, info := range infos {
		digest := info.Summary.Digest
		if _, ok := installed[digest]; !ok || info.Origin != nil && info.Origin.Kind == "bundled" {
			installed[digest] = info
		}
	}
	incomplete := false
	for _, info := range installed {
		reference := plugins.PackageReference(info)
		requirements, err := node.checkInstalledPackage(ctx, info)
		if err != nil && startup && info.Origin != nil && info.Origin.Kind == "bundled" && ctx.Err() == nil {
			if _, restoreErr := node.plugins.manager.RestoreBundled(ctx, reference); restoreErr == nil {
				requirements, err = node.checkInstalledPackage(ctx, info)
			} else {
				logUnavailablePackage(reference, "bundled plugin restoration failed", restoreErr)
			}
		}
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			incomplete = true
			if _, known := unavailable[reference.Digest]; !known {
				logUnavailablePackage(reference, "installed plugin package is unavailable", err)
			}
			unavailable[reference.Digest] = struct{}{}
			continue
		}
		delete(unavailable, reference.Digest)
		if requirements != nil {
			retained = append(retained, *requirements)
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	node.plugins.mu.Lock()
	node.plugins.unavailable = unavailable
	node.plugins.mu.Unlock()
	if incomplete {
		node.plugins.python.SuspendCollection()
	} else {
		node.plugins.python.Retain(retained)
	}
	return nil
}

func (node *Node) checkInstalledPackage(ctx context.Context, info protocol.PluginInfo) (*python.Requirements, error) {
	if info.Skill != nil {
		return nil, node.plugins.manager.VerifyPackage(ctx, plugins.PackageReference(info))
	}
	return node.packageRequirements(ctx, info)
}

func logUnavailablePackage(reference protocol.PluginReference, message string, err error) {
	fault := nodeFault(err).(*protocol.Fault)
	code, reason := fault.Code, fault.Message
	if errors.Is(err, fs.ErrNotExist) {
		code, reason = protocol.ErrorCodeNotFound, "plugin package file is missing"
	} else if errors.Is(err, fs.ErrPermission) {
		code, reason = protocol.ErrorCodePermissionDenied, "plugin package file cannot be accessed"
	}
	slog.Warn(message, "plugin", reference.Name, "digest", reference.Digest, "code", code, "reason", reason)
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
