package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginCallbackOutlivesSourceSession(t *testing.T) {
	t.Parallel()
	f := newDispatchScopeFixture(t)
	ctx := t.Context()
	var callback protocol.Request
	err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
		f.info.Extension.Actions = append(f.info.Extension.Actions,
			protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage,
			protocol.PluginActionNotify, protocol.PluginActionReadFiles, protocol.PluginActionControlConversation)
		body, err := encode(f.info)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE plugins SET body=?2 WHERE name=?1", f.info.Summary.Name, body); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE plugin_packages SET body=?2 WHERE digest=?1", f.info.Summary.Digest, body); err != nil {
			return err
		}
		config := f.handler.Callback.Command.Data.(*protocol.ConversationStart).Config
		created, _, err := createSession(ctx, db, f.node, f.storage.Profile, protocol.CommandCreateSessionPayload{Project: &f.project, Worktree: &f.worktree, Config: config})
		if err != nil {
			return err
		}
		session := created.Data.(protocol.Session)
		f.scope.Session = &session.ID
		f.handler.Callback = protocol.DispatchCallback{
			Scope:      protocol.DispatchScope{Worktree: &f.worktree, Session: &session.ID},
			Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{},
			Command: &protocol.Command{Kind: "call_plugin", Data: &protocol.CommandCallPluginPayload{Handler: "notify", Input: json.RawMessage("null")}},
		}
		if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "save_handler", Data: &f.handler}); err != nil {
			return err
		}
		if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "enqueue", Data: protocol.DispatchCommandEnqueuePayload{Handler: f.handler.Name, Payload: json.RawMessage("null")}}); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE sessions SET state='removed' WHERE id=?1", session.ID.String()); err != nil {
			return err
		}
		batch, _, err := DispatchTick(ctx, db, f.node, f.now.UnixMilli(), false)
		if err != nil {
			return err
		}
		if len(batch.Work) != 1 {
			return fmt.Errorf("callback was not claimed: %+v", batch)
		}
		callback = batch.Work[0].Request
		if callback.Plugin == nil || callback.Plugin.Session == nil || *callback.Plugin.Session != session.ID ||
			callback.Plugin.Worktree == nil || *callback.Plugin.Worktree != f.worktree {
			return fmt.Errorf("callback lost its captured bounds: %+v", callback.Plugin)
		}
		if _, err := AuthorizePluginRequest(ctx, db, f.node, f.node, callback, nil); err != nil {
			return fmt.Errorf("package callback depended on its deleted source: %w", err)
		}
		// The callback issues its own requests under the same bounds.
		callback.Plugin.Invocation = &callback.ID
		read := callback
		read.Command = protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "reminder/due"}}
		if err := CheckPluginRequest(ctx, db, read, f.info); err != nil {
			return err
		}
		if _, err := ReadPluginCommand(ctx, db, read); err != nil {
			return err
		}
		notice := protocol.NotificationDraft{Title: "Due reminder", Kind: protocol.NotificationKindInfo}
		transaction := callback
		transaction.Command = protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: []protocol.PluginTransactionOperation{
			{Kind: "write", Data: &protocol.PluginTransactionOperationWritePayload{Key: "reminder/due", Value: json.RawMessage(`{"notified":true}`)}},
			{Kind: "notify", Data: &notice},
		}}}
		if err := CheckPluginRequest(ctx, db, transaction, f.info); err != nil {
			return err
		}
		if _, _, err := ExecutePluginStorageCommand(ctx, db, f.node, f.storage.Profile, f.node, transaction); err != nil {
			return err
		}
		entry, err := ReadPluginValue(ctx, db, f.scope.Package, "reminder/due")
		if err != nil || !entry.Present || entry.Revision != 1 {
			return fmt.Errorf("notification transaction did not retain its package write: %+v: %v", entry, err)
		}
		notices, err := ReadNotifications(ctx, db)
		if err != nil || len(notices) != 1 || notices[0].Content.Title != notice.Title || notices[0].Content.Session != nil {
			return fmt.Errorf("unlinked notification was not delivered: %+v: %v", notices, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}

	for _, kind := range []string{"read_file", "linked_notification", "conversation_transaction"} {
		t.Run(kind, func(t *testing.T) {
			err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
				request := callback
				switch kind {
				case "read_file":
					request.Command = protocol.Command{Kind: "read_file", Data: &protocol.CommandReadFilePayload{Worktree: f.worktree, Path: "private.txt"}}
				case "linked_notification":
					request.Command = protocol.Command{Kind: "publish_notification", Data: &protocol.CommandPublishNotificationPayload{
						Package: f.scope.Package, Content: protocol.NotificationDraft{Title: "Source", Kind: protocol.NotificationKindInfo, Session: f.scope.Session},
					}}
				case "conversation_transaction":
					request.Command = protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: []protocol.PluginTransactionOperation{
						{Kind: "write", Data: &protocol.PluginTransactionOperationWritePayload{Key: "must-rollback", Value: json.RawMessage("true")}},
						{Kind: "conversation_write", Data: &protocol.PluginTransactionOperationConversationWritePayload{Key: "source", Value: json.RawMessage("true")}},
					}}}
				}
				if err := CheckPluginRequest(ctx, db, request, f.info); err != nil {
					return err
				}
				if kind == "conversation_transaction" {
					_, _, err := ExecutePluginStorageCommand(ctx, db, f.node, f.storage.Profile, f.node, request)
					return err
				}
				return nil
			})
			if !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				t.Fatalf("source resource access survived source removal: %v", err)
			}
		})
	}
	if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
		entry, err := ReadPluginValue(ctx, db, f.scope.Package, "must-rollback")
		if err == nil && entry.Present {
			return fmt.Errorf("rejected nested resource access committed a preceding write: %+v", entry)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPluginPackageActionsKeepPlanRestrictions(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, _ := agentHistoryFixture(t, protocol.WorkModePlan)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("plan-package-fixture", 'c')
		info.Summary.Enabled = true
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		body, err := encode(info)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)", info.Summary.Name, body); err != nil {
			return err
		}
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Worktree, scope.Session, scope.Turn = dispatchReference(info.Summary), &session.Worktree, &session.ID, &turn
		read := protocol.Request{Plugin: &scope, Command: protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "reminder/due"}}}
		if err := CheckPluginRequest(ctx, db, read, info); err != nil {
			return fmt.Errorf("planning package read failed: %w", err)
		}
		read.Command = protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{Key: "reminder/due", Value: json.RawMessage("true")}}
		if err := CheckPluginRequest(ctx, db, read, info); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("planning package write was admitted: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
