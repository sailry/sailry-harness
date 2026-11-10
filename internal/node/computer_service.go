package node

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func computerCommand(kind string) bool {
	return kind == "read_computer_permissions" || kind == "request_computer_permission"
}

// dispatchComputer reads or requests the computer helper's system grants.
// Both commands run the helper and leave no durable record.
func (node *Node) dispatchComputer(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		value, err := node.computerPermissions(ctx, caller, request)
		completion <- response(protocol.Output{Kind: "computer_permissions", Data: value}, err)
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func (node *Node) computerPermissions(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.ComputerPermissions, error) {
	probe := computer.NewProbe()
	input, requested := request.Command.Data.(*protocol.CommandRequestComputerPermissionPayload)
	if requested && caller != node.ID() {
		return probe.Request(ctx, node.ID(), caller, input.Permission, nil)
	}
	var launch *computer.Invocation
	if probe.Gated() {
		if request.Plugin == nil {
			return protocol.ComputerPermissions{}, computer.Unconfigured()
		}
		prepared, err := node.computerLaunch(ctx, request.Plugin.Package)
		if err != nil {
			return protocol.ComputerPermissions{}, err
		}
		defer func() { _ = prepared.Release() }()
		launch = &computer.Invocation{
			Command:     prepared.Launch.Command,
			Directory:   prepared.Launch.Directory,
			Env:         prepared.Launch.Env,
			Environment: prepared.Launch.Environment,
		}
	}
	if requested {
		return probe.Request(ctx, node.ID(), caller, input.Permission, launch)
	}
	return probe.Read(ctx, node.ID(), caller, launch)
}

// computerLaunch captures the helper launch of the calling package: its one
// stdio server that names the helper token. Diagnostics reuse that launch
// without joining the cursor overlay.
func (node *Node) computerLaunch(ctx context.Context, reference protocol.PluginReference) (mcp.Configuration, error) {
	info, settings, configuration, err := node.capturedPluginMCP(ctx, reference, true)
	if err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeRevisionConflict {
			return mcp.Configuration{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "computer permission package changed"}
		}
		return mcp.Configuration{}, err
	}
	name, found := "", false
	for candidate, server := range configuration.Servers {
		if !computerServer(server) {
			continue
		}
		if found {
			return mcp.Configuration{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "computer helper declaration changed"}
		}
		name, found = candidate, true
	}
	if !found {
		return mcp.Configuration{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "computer helper declaration is unavailable"}
	}
	return node.prepareMCPLaunch(ctx, info, settings, name, configuration.Servers[name])
}

func computerServer(server protocol.PluginMcpServer) bool {
	stdio, declared := server.Data.(*protocol.PluginMcpServerStdioPayload)
	return declared && stdio.Command == computer.Token
}

// startComputer finishes a helper MCP launch: it joins the package's cursor
// overlay session when the package allows one, and starts the helper under
// its own system identity.
func (node *Node) startComputer(info protocol.PluginInfo, settings store.ResolvedPluginSettings, server protocol.PluginMcpServer, prepared *mcp.Configuration) {
	if !computerServer(server) {
		return
	}
	launch := &prepared.Launch
	allowed := info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible" && computer.OverlayEnabled(settings.Values)
	session, attached := node.computerOverlay.Attach(info.Summary.Name, allowed, computer.Invocation{
		Command: launch.Command, Directory: launch.Directory, Env: launch.Env, Environment: launch.Environment,
	})
	if attached {
		launch.Env[computer.SessionVariable] = session
	}
	launch.Command, launch.Args = computer.Disclaim(launch.Command, launch.Args)
}

// retainComputerOverlay ends a changed package's overlay session once the
// package is removed, disabled or no longer allows the overlay.
func (node *Node) retainComputerOverlay(name string) {
	if node.begin() != nil {
		return
	}
	go func() {
		defer node.work.Done()
		if !node.computerOverlay.Tracks(name) {
			return
		}
		ctx := context.WithoutCancel(node.ctx)
		allowed := false
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			info, err := store.ReadPlugin(ctx, db, name)
			if err != nil || info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
				return err
			}
			settings, err := store.ResolvePluginSettings(ctx, db, plugins.PackageReference(*info))
			allowed = err == nil && computer.OverlayEnabled(settings.Values)
			return err
		})
		if err != nil || allowed {
			return
		}
		node.computerOverlay.Retain(name, nil)
	}()
}
