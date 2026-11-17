package node

import (
	"context"
	"database/sql"
	"errors"
	"log/slog"
	"runtime"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
)

func terminalCommand(kind string) bool {
	switch kind {
	case "create_terminal", "open_tool_terminal", "open_terminal", "list_terminal_tools", "close_terminal", "list_terminals", "inspect_terminal", "claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance",
		"read_terminal_settings", "save_terminal_settings", "list_shells", "open_ssh_terminal", "close_ssh_terminal", "list_ssh_terminals":
		return true
	}
	return false
}

func (node *Node) initTerminals(ctx context.Context) error {
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.RecoverTerminals(ctx, db) }); err != nil {
		return err
	}
	node.terminals = terminal.New(node.ctx, node.ID(), node.terminalChangedBy)
	return nil
}

func (node *Node) terminalChangedBy(ctx context.Context, info protocol.TerminalInfo) {
	if err := node.begin(); err != nil {
		return
	}
	defer node.work.Done()
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		changes, err := store.SaveTerminal(ctx, db, info, true)
		if err == nil {
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.publish()
	} else if ctx.Err() == nil {
		slog.Error("terminal inventory update failed", "error", err)
	}
}

func (node *Node) terminalIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, effect *workspaceCompletion) (protocol.Output, error) {
	service := node.terminals
	if service == nil {
		return protocol.Output{}, unavailable()
	}
	switch request.Command.Kind {
	case "read_terminal_settings":
		settings, err := node.terminalSettings(ctx)
		return protocol.Output{Kind: "terminal_settings", Data: settings}, err
	case "list_shells":
		var distributions []protocol.WslDistribution
		if runtime.GOOS == "windows" {
			var err error
			if distributions, err = node.wsl.List(ctx, node.environment.TerminalValues()); err != nil {
				return protocol.Output{}, err
			}
		}
		return protocol.Output{Kind: "shells", Data: terminal.Shells(node.environment.TerminalValues(), distributions)}, nil
	}
	var info protocol.TerminalInfo
	var err error
	switch input := request.Command.Data.(type) {
	case *protocol.TerminalSettings:
		effect.commit = func(ctx context.Context, db *sql.Conn, _ protocol.Output) (protocol.Output, []protocol.Event, error) {
			saved, changes, err := store.SaveTerminalSettings(ctx, db, *input)
			return protocol.Output{Kind: "terminal_settings", Data: saved}, changes, err
		}
		return protocol.Output{}, nil
	case *protocol.TerminalLaunch:
		if info, err = node.startTerminal(ctx, terminal.Launch{ID: protocol.TerminalID(request.ID), Caller: caller, Worktree: input.Worktree, Access: input.Access, Viewport: input.Viewport, Appearance: input.Appearance}); err != nil {
			return protocol.Output{}, err
		}
	case *protocol.CommandListTerminalToolsPayload:
		root, err := node.workspaceRoot(ctx, input.Worktree, false)
		if err != nil {
			return protocol.Output{}, err
		}
		settings, err := node.terminalSettings(ctx)
		if err != nil {
			return protocol.Output{}, err
		}
		return protocol.Output{Kind: "terminal_tools", Data: service.Tools(root, terminal.Environment(node.environment.TerminalValues(), settings))}, nil
	case *protocol.CommandListSshTerminalsPayload:
		var values []protocol.TerminalInfo
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			values, err = store.ListSshTerminals(ctx, db, input.Profile, caller)
			return err
		})
		return protocol.Output{Kind: "terminals", Data: values}, err
	case *protocol.CommandOpenSshTerminalPayload:
		outcome, err := node.openSSHTerminal(ctx, caller, request, *input)
		if opened, valid := outcome.Data.(protocol.TerminalInfo); valid && err == nil {
			// The remote shell runs whether or not its row is stored.
			effect.external = true
			effect.commit = func(ctx context.Context, db *sql.Conn, output protocol.Output) (protocol.Output, []protocol.Event, error) {
				changes, err := store.SaveTerminal(ctx, db, opened, false)
				return output, changes, err
			}
		}
		return protocol.Output{Kind: "ssh_outcome", Data: outcome}, err
	case *protocol.CommandCloseSshTerminalPayload:
		info, err = service.CloseTerminal(nil, input.Terminal)
	case *protocol.CommandListTerminalsPayload:
		var values []protocol.TerminalInfo
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			snapshot, err := store.Snapshot(ctx, db, node.ID())
			values = []protocol.TerminalInfo{}
			for _, value := range snapshot.Terminals {
				if value.Worktree != nil && *value.Worktree == input.Worktree {
					values = append(values, value)
				}
			}
			return err
		})
		return protocol.Output{Kind: "terminals", Data: values}, err
	case *protocol.CommandOpenToolTerminalPayload:
		if info, err = node.startTerminal(ctx, terminal.Launch{ID: protocol.TerminalID(request.ID), Caller: caller, Worktree: input.Launch.Worktree, Tool: &input.Tool, Access: input.Launch.Access, Viewport: input.Launch.Viewport, Appearance: input.Launch.Appearance}); err != nil {
			return protocol.Output{}, err
		}
	case *protocol.CommandOpenTerminalPayload:
		info, err = service.Info(input.Terminal)
	case *protocol.CommandInspectTerminalPayload:
		snapshot, err := service.Snapshot(input.Terminal)
		return protocol.Output{Kind: "terminal_snapshot", Data: snapshot}, err
	case *protocol.CommandClaimTerminalPayload:
		info, err = service.Claim(caller, input.Terminal, input.ExpectedRevision)
	case *protocol.CommandInputTerminalPayload:
		info, err = service.Input(caller, *input)
		return protocol.Output{Kind: "terminal_input", Data: protocol.OutputTerminalInputPayload{Terminal: info.ID}}, err
	case *protocol.CommandResizeTerminalPayload:
		info, err = service.Resize(caller, *input)
	case *protocol.CommandSetTerminalAppearancePayload:
		info, err = service.Appearance(caller, *input)
	case *protocol.CommandCloseTerminalPayload:
		info, err = service.CloseTerminal(&input.Worktree, input.Terminal)
	default:
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "terminal command is invalid"}
	}
	if err != nil {
		return protocol.Output{}, err
	}
	// The terminal was started, changed or closed already: a row that
	// could not be stored leaves an outcome to inspect, not an unapplied
	// request.
	effect.external = true
	effect.commit = func(ctx context.Context, db *sql.Conn, output protocol.Output) (protocol.Output, []protocol.Event, error) {
		changes, err := store.SaveTerminal(ctx, db, info, false)
		return output, changes, err
	}
	return protocol.Output{Kind: "terminal", Data: info}, nil
}

func (node *Node) terminalSettings(ctx context.Context) (protocol.TerminalSettings, error) {
	var settings protocol.TerminalSettings
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		settings, err = store.ReadTerminalSettings(ctx, db)
		return err
	})
	return settings, err
}

// startTerminal starts a local terminal in its worktree with the Node's
// terminal environment and the saved shell and variables.
func (node *Node) startTerminal(ctx context.Context, launch terminal.Launch) (protocol.TerminalInfo, error) {
	root, err := node.workspaceRoot(ctx, launch.Worktree, false)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	if launch.Settings, err = node.terminalSettings(ctx); err != nil {
		return protocol.TerminalInfo{}, err
	}
	launch.Root, launch.Environment, launch.Sandbox = root, node.environment.TerminalValues(), node.processes.SandboxTerminal
	return node.terminals.Start(ctx, launch)
}

// sshTerminalTimeout bounds connecting and requesting the remote shell.
const sshTerminalTimeout = 30 * time.Second

// openSSHTerminal connects to a saved profile and starts its shell as a
// terminal. The terminal owns the connection from then on; cancel_ssh from
// the same caller stops the opening.
func (node *Node) openSSHTerminal(ctx context.Context, caller protocol.NodeID, request protocol.Request, input protocol.CommandOpenSshTerminalPayload) (protocol.SshOutcome, error) {
	if err := terminal.ValidateViewport(input.Launch.Viewport); err != nil {
		return protocol.SshOutcome{}, err
	}
	profile, credential, err := node.readSSH(ctx, input.Profile, input.ExpectedRevision)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	scoped, finish, err := node.hostInstalls.Begin(ctx, caller, request)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	defer finish()
	timed, cancel := context.WithTimeout(scoped, sshTerminalTimeout)
	defer cancel()
	interrupted := func(failure error) error {
		return sshInterrupted(ctx, timed, failure, "SSH terminal opening cancelled; inspect terminals before retrying", "SSH terminal opening interrupted; inspect terminals before retrying", "SSH terminal opening timed out; inspect the remote process before retrying")
	}
	connection, err := ssh.ConnectWithRecovery(timed, clockwork.NewRealClock(), profile, credential)
	if err != nil {
		var required *ssh.HostKeyRequired
		if errors.As(err, &required) {
			return required.Outcome(), nil
		}
		return protocol.SshOutcome{}, sshConnectionFailure(timed, err)
	}
	// Session requests have no cancellation; closing the connection releases them.
	stop := context.AfterFunc(timed, func() { _ = connection.Close() })
	shell, err := terminal.OpenSSH(connection, input.Launch.Viewport)
	if err != nil {
		_ = connection.Close()
		return protocol.SshOutcome{}, interrupted(err)
	}
	if !stop() {
		_ = shell.Close()
		return protocol.SshOutcome{}, interrupted(context.Cause(timed))
	}
	info, err := node.terminals.Start(ctx, terminal.Launch{
		ID: protocol.TerminalID(request.ID), Caller: caller, SSH: &input.Profile, Connection: shell,
		Viewport: input.Launch.Viewport, Appearance: input.Launch.Appearance,
	})
	if err != nil {
		_ = shell.Close()
		return protocol.SshOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "SSH shell opened but terminal registration failed: " + err.Error()}
	}
	return protocol.SshOutcome{Kind: "terminal", Data: info}, nil
}

func (node *Node) subscribeTerminal(topic protocol.Topic) (link.Subscription, error) {
	switch id := topic.Data.(type) {
	case protocol.TerminalID:
		return node.terminals.Subscribe(id)
	case *protocol.TerminalID:
		if id != nil {
			return node.terminals.Subscribe(*id)
		}
	}
	return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "terminal subscription requires a terminal"}
}
