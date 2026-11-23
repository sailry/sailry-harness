package node

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"slices"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const nativeComputerOverlayOwner = "\x00native-computer"

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
		if request.Plugin != nil {
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
		} else {
			nativeLaunch, err := node.nativeComputerInvocation()
			if err != nil {
				return protocol.ComputerPermissions{}, err
			}
			launch = &nativeLaunch
		}
	}
	if requested {
		return probe.Request(ctx, node.ID(), caller, input.Permission, launch)
	}
	return probe.Read(ctx, node.ID(), caller, launch)
}

func (node *Node) nativeComputerHome() (dataDir, homeDir string, err error) {
	dataDir = filepath.Join(node.profile.Ownership.Path, "modules", "computer")
	if err = os.MkdirAll(dataDir, 0o700); err != nil {
		return "", "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "computer helper data directory is unavailable"}
	}
	homeDir = filepath.Join(dataDir, "agent-desktop")
	if err = computer.PrepareHome(dataDir, homeDir); err != nil {
		return "", "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "computer helper home is unavailable"}
	}
	return dataDir, homeDir, nil
}

func (node *Node) nativeComputerInvocation() (computer.Invocation, error) {
	exe, err := computer.Executable()
	if err != nil {
		return computer.Invocation{}, err
	}
	dataDir, homeDir, err := node.nativeComputerHome()
	if err != nil {
		return computer.Invocation{}, err
	}
	env := map[string]string{"AGENT_DESKTOP_HOME": homeDir}
	computer.Session(env)
	environment := slices.Clone(node.environment.TerminalValues())
	slices.Sort(environment)
	return computer.Invocation{
		Command:     exe,
		Directory:   dataDir,
		Env:         env,
		Environment: environment,
	}, nil
}

func (node *Node) nativeComputerAvailable() (bool, string) {
	if _, err := computer.Executable(); err != nil {
		return false, err.Error()
	}
	return true, ""
}

func (node *Node) stopNativeComputerOverlay(ctx context.Context) {
	if node.begin() != nil {
		return
	}
	owners := []string{"local\x00" + nativeComputerOverlayOwner}
	if user, ok := store.UserFromContext(ctx); ok && user.ID != (protocol.UserID{}) {
		owners = append(owners, user.ID.String()+"\x00"+nativeComputerOverlayOwner)
	}
	go func() {
		defer node.work.Done()
		for _, owner := range owners {
			if node.computerOverlay.Tracks(owner) {
				node.computerOverlay.Retain(owner, nil)
			}
		}
	}()
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
// overlay session, which an enabled, compatible package always has (there
// is no setting for it), and starts the helper under its own system
// identity.
func (node *Node) startComputer(info protocol.PluginInfo, server protocol.PluginMcpServer, prepared *mcp.Configuration) {
	if !computerServer(server) {
		return
	}
	launch := &prepared.Launch
	allowed := info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible"
	session, attached := node.computerOverlay.Attach(prepared.User+"\x00"+info.Summary.Name, allowed, computer.Invocation{
		Command: launch.Command, Directory: launch.Directory, Env: launch.Env, Environment: launch.Environment,
	})
	if attached {
		launch.Env[computer.SessionVariable] = session
	}
	launch.Command, launch.Args = computer.Disclaim(launch.Command, launch.Args)
}

// retainComputerOverlay ends a changed package's overlay session once the
// package is removed or disabled.
func (node *Node) retainComputerOverlay(name string, contexts ...context.Context) {
	ctx := node.ctx
	if len(contexts) != 0 {
		ctx = contexts[0]
	}
	user, err := node.pluginRuntimeUser(ctx)
	if err != nil {
		return
	}
	owner := pluginProgramOwner(user, name)
	ctx = context.WithoutCancel(ctx)
	if node.begin() != nil {
		return
	}
	go func() {
		defer node.work.Done()
		if !node.computerOverlay.Tracks(owner) {
			return
		}
		allowed := false
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			info, err := store.ReadPlugin(ctx, db, name)
			allowed = err == nil && info != nil && info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible"
			return err
		})
		if err != nil || allowed {
			return
		}
		node.computerOverlay.Retain(owner, nil)
	}()
}
