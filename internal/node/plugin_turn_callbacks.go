package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"log/slog"
	"maps"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// afterTurn calls each captured package's after_turn handler for an ended
// main task turn. It is an ordinary plugin callback dispatched for the turn's
// caller, so the callback runtime's budget, deadline and cancellation apply
// and the handler holds exactly the package's declared authority.
//
// The input states the turn's facts so a package loop need not page history
// for them: its place in the session's order, how it ended, what started it,
// the tools the model called,
// the interactions that expired, its usage and the session's, the background
// work still running or waiting, and the automatic chain.
func (node *Node) afterTurn(ctx context.Context, inv *store.AgentInvocation, run protocol.ConversationRun, observed engine.TurnObservation) {
	var facts store.TurnFacts
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		facts, err = store.ReadTurnFacts(ctx, db, inv.Session.ID, run.Turn)
		return err
	}); err != nil {
		logging.Emit(slog.LevelWarn, slog.String("area", "plugins"), slog.String("phase", logging.Failed), slog.String("operation", "after_turn"),
			slog.String("session", inv.Session.ID.String()), slog.String("turn", run.Turn.String()), slog.String("exception", err.Error()))
		return
	}
	tools := map[string]int{}
	calls := 0
	for name, count := range observed.Tools {
		tools[name] = count
		calls += count
	}
	base := map[string]any{
		"turn": run.Turn, "sequence": run.Sequence, "status": run.Status, "automatic": run.Notice || run.Continuation != nil, "notice": run.Notice,
		"mode": inv.Turn.Config.Mode, "tool_calls": calls, "tools": tools, "expired": observed.Expired,
		"usage": facts.Usage, "session_usage": facts.SessionUsage,
		"background": map[string]int{"running": facts.Running, "pending": facts.Pending},
		"chain":      map[string]int{"count": facts.Chain.Count, "limit": facts.Chain.Limit},
	}
	if run.Error != nil {
		base["error"] = run.Error.Code
	}
	for _, reference := range inv.Turn.Plugins {
		var info protocol.PluginInfo
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			info, err = store.ReadPluginVersion(ctx, db, reference)
			return err
		})
		if err != nil || info.Extension == nil || info.Extension.Host == nil || info.Extension.Host.Turn == nil || info.Extension.Host.Turn.AfterTurn == nil {
			continue
		}
		names := []string{}
		for _, tool := range info.Extension.Tools {
			names = append(names, plugins.ToolAlias(info.Summary.Name, tool.Name))
		}
		fields := maps.Clone(base)
		fields["names"] = names
		// A package sees only its own continuation; another package's is an
		// automatic turn like any other.
		if run.Continuation != nil && run.Continuation.Package.Name == info.Summary.Name {
			fields["continuation"] = map[string]any{"marker": run.Continuation.Kind}
		}
		input, err := json.Marshal(fields)
		if err != nil {
			continue
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: *info.Extension.Host.Turn.AfterTurn, Input: input}})
		request.Plugin = &protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "workspace"}, Package: plugins.PackageReference(info), Worktree: new(inv.Run.Worktree), Session: new(inv.Session.ID)}
		var failure *protocol.Fault
		accepted, err := node.Dispatch(ctx, inv.Caller, request)
		if err != nil {
			failure = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: err.Error()}
			if value, ok := err.(*protocol.Fault); ok {
				failure = value
			}
		} else {
			select {
			case <-ctx.Done():
				return
			case result, open := <-accepted.Completion:
				if !open {
					failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "turn callback completion is unknown"}
				} else {
					failure = result.Fault
				}
			}
		}
		// A package that was disabled, removed or changed since the turn
		// captured it simply has no callback any more.
		if failure == nil || failure.Code == protocol.ErrorCodeNotConfigured || failure.Code == protocol.ErrorCodeNotFound || failure.Code == protocol.ErrorCodeRevisionConflict {
			continue
		}
		logging.Emit(slog.LevelWarn, slog.String("area", "plugins"), slog.String("phase", logging.Failed), slog.String("operation", "after_turn"),
			slog.String("package", info.Summary.Name), slog.String("session", inv.Session.ID.String()), slog.String("turn", inv.Run.Turn.String()), slog.String("exception", failure.Message))
	}
}
