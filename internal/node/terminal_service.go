package node

import (
	"context"
	"database/sql"
	"log/slog"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func terminalCommand(kind string) bool {
	switch kind {
	case "open_tool_terminal", "open_terminal", "list_terminal_tools", "close_terminal", "list_terminals", "inspect_terminal", "claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance":
		return true
	}
	return false
}

func (node *Node) initTerminals(ctx context.Context) error {
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.RecoverTerminals(ctx, db) }); err != nil {
		return err
	}
	node.terminals = terminal.New(node.ctx, node.ID(), node.terminalChanged)
	return nil
}

func (node *Node) terminalChanged(info protocol.TerminalInfo) {
	if err := node.begin(); err != nil {
		return
	}
	defer node.work.Done()
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		changes, err := store.SaveTerminal(node.ctx, db, info, true)
		if err == nil {
			_, err = store.RecordEvents(node.ctx, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.publish()
	} else if node.ctx.Err() == nil {
		slog.Error("terminal inventory update failed", "error", err)
	}
}

func (node *Node) terminalIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, effect *workspaceCompletion) (protocol.Output, error) {
	if request.Plugin != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "terminal commands require a controller"}
	}
	service := node.terminals
	if service == nil {
		return protocol.Output{}, unavailable()
	}
	var info protocol.TerminalInfo
	var err error
	switch input := request.Command.Data.(type) {
	case *protocol.CommandListTerminalToolsPayload:
		root, err := node.workspaceRoot(ctx, input.Worktree, false)
		if err != nil {
			return protocol.Output{}, err
		}
		return protocol.Output{Kind: "terminal_tools", Data: service.Tools(root, node.environment.TerminalValues())}, nil
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
		root, err := node.workspaceRoot(ctx, input.Launch.Worktree, false)
		if err != nil {
			return protocol.Output{}, err
		}
		info, err = service.Start(ctx, terminal.Launch{ID: protocol.TerminalID(request.ID), Caller: caller, Worktree: input.Launch.Worktree, Root: root, Tool: &input.Tool, Access: input.Launch.Access, Environment: node.environment.TerminalValues(), Viewport: input.Launch.Viewport, Appearance: input.Launch.Appearance})
		if err != nil {
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
		info, err = service.CloseTerminal(input.Worktree, input.Terminal)
		if err != nil {
			var saved protocol.TerminalInfo
			readErr := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				saved, err = store.ReadTerminal(ctx, db, input.Terminal)
				return err
			})
			if readErr == nil && saved.Worktree != nil && *saved.Worktree == input.Worktree && saved.Status.Kind != "running" {
				saved.Status, saved.Owner = protocol.TerminalStatus{Kind: "closed"}, nil
				saved.Revision++
				info, err = saved, nil
			}
		}
	default:
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "terminal command is invalid"}
	}
	if err != nil {
		return protocol.Output{}, err
	}
	effect.commit = func(ctx context.Context, db *sql.Conn, output protocol.Output) (protocol.Output, []protocol.Event, error) {
		changes, err := store.SaveTerminal(ctx, db, info, false)
		return output, changes, err
	}
	return protocol.Output{Kind: "terminal", Data: info}, nil
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
