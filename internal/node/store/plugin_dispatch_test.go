package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func dispatchTriggerAction(kind string, handler protocol.DispatchHandler) protocol.DispatchCommand {
	switch kind {
	case "enqueue":
		return protocol.DispatchCommand{Kind: kind, Data: protocol.DispatchCommandEnqueuePayload{Handler: handler.Name, Payload: []byte("null")}}
	case "publish":
		return protocol.DispatchCommand{Kind: kind, Data: protocol.DispatchCommandPublishPayload{Topic: handler.Source.Topic, Payload: []byte("null")}}
	default:
		return protocol.DispatchCommand{Kind: "save_schedule", Data: protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Enabled: true, Topic: handler.Source.Topic,
			Payload: []byte("null"), Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1}}}}
	}
}

func TestPluginDispatchTriggersKeepCapturedScope(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"enqueue", "publish", "save_schedule"} {
		for _, scenario := range []string{"same", "node_wide_handler", "other_worktree", "missing_session", "other_session", "node_wide_caller"} {
			t.Run(kind+"/"+scenario, func(t *testing.T) {
				t.Parallel()
				f := newDispatchScopeFixture(t)
				ctx := t.Context()
				err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
					sessions, err := ReadSessions(ctx, db)
					if err != nil || len(sessions) != 1 {
						return fmt.Errorf("fixture source session: %d, %v", len(sessions), err)
					}
					source := sessions[0]
					f.scope.Session = &source.ID
					handler := f.handler
					handler.Callback.Scope.Session = &source.ID
					handler.Callback.Bindings = nil
					handler.Callback.Completion = protocol.DispatchCompletionCommand
					handler.Callback.Command = &protocol.Command{Kind: "read_plugin_value", Data: protocol.CommandReadPluginValuePayload{Key: "fixture"}}
					switch scenario {
					case "node_wide_handler":
						handler.Callback.Scope = protocol.DispatchScope{}
					case "other_worktree":
						handler.Callback.Scope.Worktree = &f.other
					case "missing_session":
						handler.Callback.Scope.Session = nil
					case "other_session":
						other := protocol.NewSessionID()
						handler.Callback.Scope.Session = &other
					case "node_wide_caller":
						f.scope.Session, f.scope.Worktree = nil, nil
					}
					// The existing handler may have been created by a controller;
					// the new trigger must still respect its own captured scope.
					if _, err := saveDispatchHandler(ctx, db, f.scope.Package, handler); err != nil {
						return err
					}
					action := dispatchTriggerAction(kind, handler)
					request := protocol.Request{Plugin: &f.scope, Command: protocol.Command{Kind: "dispatch", Data: &protocol.CommandDispatchPayload{Package: f.scope.Package, Action: action}}}
					err = CheckPluginRequest(ctx, db, request, f.info)
					allowed := scenario == "same" || scenario == "node_wide_caller"
					if allowed && err != nil || !allowed && !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("trigger scope allowed=%t: %w", allowed, err)
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	}
}

func TestPluginDispatchTriggersCannotBorrowSavedPermissions(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"enqueue", "publish", "save_schedule"} {
		for _, authority := range []string{"ask", "plan", "full"} {
			t.Run(kind+"/"+authority, func(t *testing.T) {
				t.Parallel()
				var storage *Store
				var session protocol.Session
				var turn protocol.TurnID
				if authority == "plan" {
					storage, _, session, turn, _ = agentHistoryFixture(t, protocol.WorkModePlan)
				} else {
					permission := protocol.PermissionAsk
					if authority == "full" {
						permission = protocol.PermissionFull
					}
					storage, _, session, turn, _ = delegationPermissionFixture(t, permission, false)
				}
				ctx := t.Context()
				err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					// The admitted turn remains Ask/Plan even if the source
					// settings are changed before this explicit trigger.
					session.Config.Permission, session.Config.Mode = protocol.PermissionFull, protocol.WorkModeCode
					if _, _, err := reviseSession(ctx, db, session); err != nil {
						return err
					}
					info := pluginTestInfo("trigger-fixture", 'e')
					info.Summary.Enabled = true
					info.Extension = new(protocol.DefaultPluginExtension())
					info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionDispatch, protocol.PluginActionStartSessions}
					scope := protocol.DefaultPluginContext()
					scope.Package = dispatchReference(info.Summary)
					scope.Worktree, scope.Session, scope.Turn = &session.Worktree, &session.ID, &turn
					draft := protocol.DefaultConversationStart()
					draft.Config, draft.Title, draft.Message.Text = &session.Config, "Previously authorized task", "Run the saved task"
					handler := protocol.DispatchHandler{Name: "full", Enabled: true, Queue: "full", Source: protocol.DispatchSource{Package: scope.Package.Name, Topic: "full"},
						Callback: protocol.DispatchCallback{Scope: protocol.DispatchScope{Worktree: scope.Worktree, Session: scope.Session}, Completion: protocol.DispatchCompletionTurn,
							Command: &protocol.Command{Kind: "start_session", Data: &draft}}}
					if _, err := saveDispatchHandler(ctx, db, scope.Package, handler); err != nil {
						return err
					}
					request := protocol.Request{Plugin: &scope, Command: protocol.Command{Kind: "dispatch", Data: &protocol.CommandDispatchPayload{Package: scope.Package, Action: dispatchTriggerAction(kind, handler)}}}
					err := CheckPluginRequest(ctx, db, request, info)
					if authority == "full" && err != nil || authority != "full" && !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("captured %s trigger: %w", authority, err)
					}
					return nil
				})
				if err != nil {
					t.Fatal(err)
				}
			})
		}
	}
}

func TestScopedDispatchScheduleRequiresItsOwnHandler(t *testing.T) {
	t.Parallel()
	f := newDispatchScopeFixture(t)
	ctx := t.Context()
	err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
		// A foreign subscriber does not grant the publisher scheduling authority.
		foreign := f.scope.Package
		foreign.Name = "foreign-subscriber"
		if _, err := saveDispatchHandler(ctx, db, foreign, f.handler); err != nil {
			return err
		}
		if err := checkPluginDispatchTargets(ctx, db, f.scope, dispatchTriggerAction("save_schedule", f.handler)); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("scoped schedule without own handler was accepted: %w", err)
		}
		// Cross-package subscriptions remain receiver-authorized, and a plain
		// publish with no local listener does not grant a durable callback.
		if err := checkPluginDispatchTargets(ctx, db, f.scope, dispatchTriggerAction("publish", f.handler)); err != nil {
			return err
		}
		if _, err := saveDispatchHandler(ctx, db, f.scope.Package, f.handler); err != nil {
			return err
		}
		return checkPluginDispatchTargets(ctx, db, f.scope, dispatchTriggerAction("save_schedule", f.handler))
	})
	if err != nil {
		t.Fatal(err)
	}
}
